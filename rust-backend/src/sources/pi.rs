use crate::{
    pricing::{model_cost_usd, TokenUsage},
    sources::{home_dir, num, to_i64},
};
use chrono::{DateTime, Local};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const PI_AGENT_DIR_ENV: &str = "PI_AGENT_DIR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceView {
    Daily,
    Monthly,
    Sessions,
    Blocks,
}

impl TryFrom<&str> for SourceView {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "daily" => Ok(Self::Daily),
            "monthly" => Ok(Self::Monthly),
            "sessions" => Ok(Self::Sessions),
            "blocks" => Ok(Self::Blocks),
            other => Err(format!("unsupported view: {other}")),
        }
    }
}

#[derive(Debug, Clone)]
struct UsageEntry {
    timestamp: DateTime<Local>,
    session_id: String,
    project_path: String,
    model_name: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_cost: f64,
}

impl UsageEntry {
    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Default, Clone)]
struct Totals {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_cost: f64,
}

impl Totals {
    fn add_entry(&mut self, entry: &UsageEntry) {
        self.input_tokens += entry.input_tokens;
        self.output_tokens += entry.output_tokens;
        self.cache_creation_tokens += entry.cache_creation_tokens;
        self.cache_read_tokens += entry.cache_read_tokens;
        self.total_cost += entry.total_cost;
    }

    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Default, Clone)]
struct Aggregate {
    totals: Totals,
    by_model: BTreeMap<String, Totals>,
}

impl Aggregate {
    fn add_entry(&mut self, entry: &UsageEntry) {
        self.totals.add_entry(entry);
        self.by_model
            .entry(entry.model_name.clone())
            .or_default()
            .add_entry(entry);
    }

    fn models_used(&self) -> Vec<String> {
        self.by_model.keys().cloned().collect()
    }

    fn model_breakdowns(&self) -> Vec<Value> {
        self.by_model
            .iter()
            .map(|(model, totals)| {
                json!({
                    "modelName": model,
                    "inputTokens": totals.input_tokens,
                    "outputTokens": totals.output_tokens,
                    "cacheCreationTokens": totals.cache_creation_tokens,
                    "cacheReadTokens": totals.cache_read_tokens,
                    "cost": totals.total_cost,
                })
            })
            .collect()
    }
}

pub fn load_source_view(view: &str, refresh: bool) -> Result<Vec<Value>, String> {
    let _ = refresh;
    let view = SourceView::try_from(view)?;

    if view == SourceView::Blocks {
        return Ok(Vec::new());
    }

    let entries = load_usage_entries()?;
    Ok(match view {
        SourceView::Daily => entries_to_daily(&entries),
        SourceView::Monthly => entries_to_monthly(&entries),
        SourceView::Sessions => entries_to_sessions(&entries),
        SourceView::Blocks => Vec::new(),
    })
}

fn load_usage_entries() -> Result<Vec<UsageEntry>, String> {
    let Some(sessions_dir) = discover_sessions_dir() else {
        return Ok(Vec::new());
    };

    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_jsonl_files(&sessions_dir, &mut files)?;
    files.sort();

    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for file in files {
        append_entries_from_file(&sessions_dir, &file, &mut seen, &mut entries)?;
    }

    entries.sort_by_key(|entry| entry.timestamp.timestamp_millis());
    Ok(entries)
}

fn discover_sessions_dir() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(PI_AGENT_DIR_ENV) {
        let path = PathBuf::from(raw);
        if path.is_dir() {
            return Some(path);
        }
    }

    home_dir().map(|home| home.join(".pi").join("agent").join("sessions"))
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(format!("failed to read {}: {err}", dir.display())),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn append_entries_from_file(
    sessions_dir: &Path,
    file_path: &Path,
    seen: &mut BTreeSet<String>,
    entries: &mut Vec<UsageEntry>,
) -> Result<(), String> {
    let file = match File::open(file_path) {
        Ok(file) => file,
        Err(_) => return Ok(()),
    };

    let session_id = extract_session_id(file_path);
    let project_path = extract_project_path(sessions_dir, file_path);

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let Some(entry) = parse_usage_entry(&value, &session_id, &project_path) else {
            continue;
        };

        let hash = format!(
            "pi:{}:{}",
            entry.timestamp.to_rfc3339(),
            entry.total_tokens()
        );
        if seen.insert(hash) {
            entries.push(entry);
        }
    }

    Ok(())
}

fn parse_usage_entry(value: &Value, session_id: &str, project_path: &str) -> Option<UsageEntry> {
    let entry_type = value.get("type").and_then(Value::as_str);
    if entry_type.is_some_and(|entry_type| entry_type != "message") {
        return None;
    }

    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)?;
    let message = value.get("message")?.as_object()?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }

    let usage = message.get("usage")?.as_object()?;
    let input = usage.get("input")?;
    let output = usage.get("output")?;
    let input_tokens = to_i64(input);
    let output_tokens = to_i64(output);
    let cache_creation_tokens = usage.get("cacheWrite").map(to_i64).unwrap_or_default();
    let cache_read_tokens = usage.get("cacheRead").map(to_i64).unwrap_or_default();
    let model_name = message
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "unknown".to_string());
    let explicit_cost = usage
        .get("cost")
        .and_then(|cost| cost.get("total"))
        .map(num)
        .unwrap_or_default();
    let total_cost = if explicit_cost > 0.0 {
        explicit_cost
    } else {
        model_cost_usd(
            &model_name,
            TokenUsage {
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            },
        )
    };

    let entry = UsageEntry {
        timestamp,
        session_id: session_id.to_owned(),
        project_path: project_path.to_owned(),
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_cost,
    };

    (entry.total_tokens() > 0 || entry.total_cost > 0.0).then_some(entry)
}

fn entries_to_daily(entries: &[UsageEntry]) -> Vec<Value> {
    aggregate_by(entries, |entry| {
        entry.timestamp.format("%Y-%m-%d").to_string()
    })
    .into_iter()
    .map(|(date, aggregate)| usage_row("date", date, aggregate))
    .collect()
}

fn entries_to_monthly(entries: &[UsageEntry]) -> Vec<Value> {
    aggregate_by(entries, |entry| entry.timestamp.format("%Y-%m").to_string())
        .into_iter()
        .map(|(month, aggregate)| usage_row("month", month, aggregate))
        .collect()
}

fn entries_to_sessions(entries: &[UsageEntry]) -> Vec<Value> {
    let mut grouped: BTreeMap<(String, String), (DateTime<Local>, Aggregate)> = BTreeMap::new();

    for entry in entries {
        let key = (entry.project_path.clone(), entry.session_id.clone());
        let (last_activity, aggregate) = grouped
            .entry(key)
            .or_insert_with(|| (entry.timestamp, Aggregate::default()));
        if entry.timestamp > *last_activity {
            *last_activity = entry.timestamp;
        }
        aggregate.add_entry(entry);
    }

    let mut rows = grouped
        .into_iter()
        .map(|((project_path, session_id), (last_activity, aggregate))| {
            json!({
                "sessionId": session_id,
                "projectPath": project_path,
                "date": last_activity.format("%Y-%m-%d").to_string(),
                "time": last_activity.format("%H:%M").to_string(),
                "inputTokens": aggregate.totals.input_tokens,
                "outputTokens": aggregate.totals.output_tokens,
                "cacheCreationTokens": aggregate.totals.cache_creation_tokens,
                "cacheReadTokens": aggregate.totals.cache_read_tokens,
                "totalTokens": aggregate.totals.total_tokens(),
                "totalCost": aggregate.totals.total_cost,
                "modelsUsed": aggregate.models_used(),
                "modelBreakdowns": aggregate.model_breakdowns(),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(sort_key);
    rows
}

fn aggregate_by(
    entries: &[UsageEntry],
    key_for: impl Fn(&UsageEntry) -> String,
) -> BTreeMap<String, Aggregate> {
    let mut grouped: BTreeMap<String, Aggregate> = BTreeMap::new();
    for entry in entries {
        grouped.entry(key_for(entry)).or_default().add_entry(entry);
    }
    grouped
}

fn usage_row(key_name: &str, key: String, aggregate: Aggregate) -> Value {
    json!({
        key_name: key,
        "inputTokens": aggregate.totals.input_tokens,
        "outputTokens": aggregate.totals.output_tokens,
        "cacheCreationTokens": aggregate.totals.cache_creation_tokens,
        "cacheReadTokens": aggregate.totals.cache_read_tokens,
        "totalTokens": aggregate.totals.total_tokens(),
        "totalCost": aggregate.totals.total_cost,
        "modelsUsed": aggregate.models_used(),
        "modelBreakdowns": aggregate.model_breakdowns(),
    })
}

fn parse_timestamp(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date_time| date_time.with_timezone(&Local))
}

fn extract_session_id(file_path: &Path) -> String {
    let stem = file_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown");
    stem.split_once('_')
        .map(|(_, session_id)| session_id)
        .unwrap_or(stem)
        .to_owned()
}

fn extract_project_path(sessions_dir: &Path, file_path: &Path) -> String {
    file_path
        .strip_prefix(sessions_dir)
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        .filter(|project| !project.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn sort_key(value: &Value) -> String {
    let date = value
        .get("date")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let time = value
        .get("time")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("{date}T{time}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_usage_entry_accepts_assistant_messages_with_usage() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "assistant",
                "model": "anthropic/claude-opus-4.5",
                "usage": {
                    "input": 100,
                    "output": 40,
                    "cacheRead": 8,
                    "cacheWrite": 12,
                    "cost": { "total": 0.25 }
                }
            }
        });

        let entry = parse_usage_entry(&raw, "session-1", "project-1").unwrap();

        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.project_path, "project-1");
        assert_eq!(entry.model_name, "anthropic/claude-opus-4.5");
        assert_eq!(entry.input_tokens, 100);
        assert_eq!(entry.output_tokens, 40);
        assert_eq!(entry.cache_read_tokens, 8);
        assert_eq!(entry.cache_creation_tokens, 12);
        assert_eq!(entry.total_cost, 0.25);
    }

    #[test]
    fn parse_usage_entry_rejects_non_assistant_messages() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "user",
                "usage": {
                    "input": 100,
                    "output": 40
                }
            }
        });

        assert!(parse_usage_entry(&raw, "session-1", "project-1").is_none());
    }

    #[test]
    fn parse_usage_entry_prices_mimo_when_cost_is_missing() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "assistant",
                "model": "mimo-v2.5-pro",
                "usage": {
                    "input": 6200,
                    "output": 33
                }
            }
        });

        let entry = parse_usage_entry(&raw, "session-1", "project-1").unwrap();

        assert!(entry.total_cost > 0.0);
    }

    #[test]
    fn entries_to_sessions_groups_project_and_session() {
        let first = parse_usage_entry(
            &json!({
                "timestamp": "2026-01-02T03:04:05Z",
                "message": {
                    "role": "assistant",
                    "model": "model-a",
                    "usage": { "input": 10, "output": 20 }
                }
            }),
            "session-1",
            "project-1",
        )
        .unwrap();
        let second = parse_usage_entry(
            &json!({
                "timestamp": "2026-01-02T04:04:05Z",
                "message": {
                    "role": "assistant",
                    "model": "model-b",
                    "usage": { "input": 5, "output": 7, "cacheRead": 3 }
                }
            }),
            "session-1",
            "project-1",
        )
        .unwrap();

        let rows = entries_to_sessions(&[first, second]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sessionId"], json!("session-1"));
        assert_eq!(rows[0]["projectPath"], json!("project-1"));
        assert_eq!(rows[0]["inputTokens"], json!(15));
        assert_eq!(rows[0]["outputTokens"], json!(27));
        assert_eq!(rows[0]["cacheReadTokens"], json!(3));
        assert_eq!(rows[0]["modelsUsed"], json!(["model-a", "model-b"]));
    }

    #[test]
    fn entries_to_daily_and_monthly_use_expected_period_keys() {
        let entry = parse_usage_entry(
            &json!({
                "timestamp": "2026-01-02T03:04:05Z",
                "message": {
                    "role": "assistant",
                    "model": "model-a",
                    "usage": { "input": 10, "output": 20 }
                }
            }),
            "session-1",
            "project-1",
        )
        .unwrap();

        let daily = entries_to_daily(std::slice::from_ref(&entry));
        let monthly = entries_to_monthly(&[entry]);

        assert_eq!(daily[0]["date"], json!("2026-01-02"));
        assert_eq!(monthly[0]["month"], json!("2026-01"));
    }

    #[test]
    fn extracts_session_id_after_timestamp_prefix() {
        let path = Path::new("/tmp/sessions/project/2025-12-19T08-12-33-794Z_2c16ab69.jsonl");

        assert_eq!(extract_session_id(path), "2c16ab69");
    }
}
