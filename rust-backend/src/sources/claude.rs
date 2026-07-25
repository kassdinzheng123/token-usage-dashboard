use crate::pricing::{model_cost_usd, TokenUsage};
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryFrom;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
const PROJECTS_DIR: &str = "projects";
const BLOCK_HOURS: i64 = 5;

#[derive(Debug, Clone)]
struct UsageCounts {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
}

impl UsageCounts {
    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Clone)]
struct UsageEntry {
    timestamp: DateTime<Local>,
    session_id: String,
    model: String,
    usage: UsageCounts,
    cost_usd: f64,
    /// Stable dedupe key (messageId[:requestId], or path:line fallback).
    message_key: String,
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
        self.input_tokens += entry.usage.input_tokens;
        self.output_tokens += entry.usage.output_tokens;
        self.cache_creation_tokens += entry.usage.cache_creation_tokens;
        self.cache_read_tokens += entry.usage.cache_read_tokens;
        self.total_cost += entry.cost_usd;
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

        if entry.model == "<synthetic>" {
            return;
        }

        self.by_model
            .entry(super::cluster_model_name(&entry.model))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceView {
    Daily,
    Monthly,
    Sessions,
    Blocks,
    Messages,
}

impl TryFrom<&str> for SourceView {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "daily" => Ok(Self::Daily),
            "monthly" => Ok(Self::Monthly),
            "sessions" => Ok(Self::Sessions),
            "blocks" => Ok(Self::Blocks),
            "messages" => Ok(Self::Messages),
            other => Err(format!("unsupported view: {other}")),
        }
    }
}

pub fn load_source_view(view: &str, refresh: bool) -> Result<Vec<Value>, String> {
    load_source_view_since(view, refresh, None)
}

pub fn load_source_view_since(
    view: &str,
    _refresh: bool,
    watermark_ms: Option<i64>,
) -> Result<Vec<Value>, String> {
    let view = SourceView::try_from(view)?;
    let entries = load_usage_entries(watermark_ms)?;

    Ok(match view {
        SourceView::Daily => entries_to_daily(&entries),
        SourceView::Monthly => entries_to_monthly(&entries),
        SourceView::Sessions => entries_to_sessions(&entries),
        SourceView::Blocks => entries_to_blocks(&entries),
        SourceView::Messages => entries_to_messages(&entries),
    })
}

pub fn load_daily_for_date(date: &str, refresh: bool) -> Result<Vec<Value>, String> {
    let _ = refresh;
    let entries = load_usage_entries_for_date(date)?;
    Ok(entries_to_daily(&entries)
        .into_iter()
        .filter(|row| row.get("date").and_then(Value::as_str) == Some(date))
        .collect())
}

fn load_usage_entries(watermark_ms: Option<i64>) -> Result<Vec<UsageEntry>, String> {
    let projects_dirs = discover_projects_dirs();
    if projects_dirs.is_empty() {
        return Ok(Vec::new());
    }

    let mut jsonl_files = Vec::new();
    for projects_dir in projects_dirs {
        match watermark_ms {
            // `collect_jsonl_files_modified_since` is inclusive; shift by 1ms
            // so files at exactly the watermark are skipped.
            Some(watermark) => collect_jsonl_files_modified_since(
                &projects_dir,
                &mut jsonl_files,
                watermark.saturating_add(1),
            )?,
            None => collect_jsonl_files(&projects_dir, &mut jsonl_files)?,
        }
    }
    jsonl_files.sort();

    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for path in jsonl_files {
        read_usage_file(&path, &mut seen, &mut entries)?;
    }
    entries.sort_by_key(|entry| entry.timestamp.timestamp_millis());
    Ok(entries)
}

fn load_usage_entries_for_date(date: &str) -> Result<Vec<UsageEntry>, String> {
    let Some((start, end)) = local_day_bounds(date) else {
        return Ok(Vec::new());
    };
    let projects_dirs = discover_projects_dirs();
    if projects_dirs.is_empty() {
        return Ok(Vec::new());
    }

    let mut jsonl_files = Vec::new();
    let start_ms = start.timestamp_millis();
    for projects_dir in projects_dirs {
        collect_jsonl_files_modified_since(&projects_dir, &mut jsonl_files, start_ms)?;
    }
    jsonl_files.sort();

    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for path in jsonl_files {
        read_usage_file(&path, &mut seen, &mut entries)?;
    }
    entries.retain(|entry| entry.timestamp >= start && entry.timestamp < end);
    entries.sort_by_key(|entry| entry.timestamp.timestamp_millis());
    Ok(entries)
}

fn discover_projects_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = BTreeSet::new();

    if let Some(raw) = std::env::var_os(CLAUDE_CONFIG_DIR) {
        for config_dir in split_env_paths(&raw.to_string_lossy()) {
            push_projects_dir(&mut dirs, &mut seen, PathBuf::from(config_dir));
        }
        return dirs;
    }

    let Some(home) = home_dir() else {
        return dirs;
    };

    push_existing_dir(
        &mut dirs,
        &mut seen,
        home.join(".config").join("claude").join(PROJECTS_DIR),
    );
    push_existing_dir(
        &mut dirs,
        &mut seen,
        home.join(".claude").join(PROJECTS_DIR),
    );
    dirs
}

fn split_env_paths(raw: &str) -> Vec<String> {
    raw.split(',')
        .flat_map(|part| std::env::split_paths(std::ffi::OsStr::new(part)))
        .filter_map(|path| {
            let text = path.to_string_lossy().trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        })
        .collect()
}

fn push_projects_dir(dirs: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>, config_dir: PathBuf) {
    if config_dir.file_name().and_then(|name| name.to_str()) == Some(PROJECTS_DIR) {
        push_existing_dir(dirs, seen, config_dir);
        return;
    }

    push_existing_dir(dirs, seen, config_dir.join(PROJECTS_DIR));
}

fn push_existing_dir(dirs: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>, path: PathBuf) {
    if !path.is_dir() {
        return;
    }

    let normalized = fs::canonicalize(&path).unwrap_or(path);
    if seen.insert(normalized.clone()) {
        dirs.push(normalized);
    }
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
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }

    Ok(())
}

fn collect_jsonl_files_modified_since(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    start_ms: i64,
) -> Result<(), String> {
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
            collect_jsonl_files_modified_since(&path, files, start_ms)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            && file_modified_at_or_after(&path, start_ms)
        {
            files.push(path);
        }
    }

    Ok(())
}

fn read_usage_file(
    path: &Path,
    seen: &mut BTreeSet<String>,
    entries: &mut Vec<UsageEntry>,
) -> Result<(), String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(format!("failed to open {}: {err}", path.display())),
    };

    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
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
        let Some(mut entry) = entry_from_value(&value, path) else {
            continue;
        };

        let dedupe_key = dedupe_key(&value)
            .unwrap_or_else(|| format!("{}:{line_number}", path.to_string_lossy()));
        if seen.insert(dedupe_key.clone()) {
            entry.message_key = dedupe_key;
            entries.push(entry);
        }
    }

    Ok(())
}

fn file_modified_at_or_after(path: &Path, start_ms: i64) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .is_some_and(|modified_ms| modified_ms >= start_ms)
}

fn entry_from_value(value: &Value, path: &Path) -> Option<UsageEntry> {
    let timestamp = value.get("timestamp").and_then(parse_timestamp)?;
    let usage = extract_usage(value)?;
    if usage.total_tokens() <= 0 {
        return None;
    }

    let model = extract_model(value).unwrap_or_else(|| "unknown".to_string());
    let explicit_cost =
        number_field(value, &["costUSD", "cost_usd", "totalCost", "cost"]).unwrap_or_default();
    let cost_usd = if explicit_cost > 0.0 {
        explicit_cost
    } else {
        model_cost_usd(
            &model,
            TokenUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cache_creation_tokens: usage.cache_creation_tokens,
                cache_read_tokens: usage.cache_read_tokens,
            },
        )
    };

    Some(UsageEntry {
        timestamp,
        session_id: string_field(value, &["sessionId", "session_id"])
            .or_else(|| session_id_from_path(path))
            .unwrap_or_else(|| "unknown".to_string()),
        model,
        usage,
        cost_usd,
        message_key: String::new(),
    })
}

fn extract_usage(value: &Value) -> Option<UsageCounts> {
    let usage = value
        .get("message")
        .and_then(|message| message.get("usage"))
        .or_else(|| value.get("usage"))?;

    Some(UsageCounts {
        input_tokens: integer_field(
            usage,
            &["input_tokens", "inputTokens", "input", "prompt_tokens"],
        )
        .unwrap_or_default(),
        output_tokens: integer_field(
            usage,
            &[
                "output_tokens",
                "outputTokens",
                "output",
                "completion_tokens",
            ],
        )
        .unwrap_or_default(),
        cache_creation_tokens: integer_field(
            usage,
            &[
                "cache_creation_input_tokens",
                "cache_creation_tokens",
                "cacheCreationInputTokens",
                "cacheCreationTokens",
                "cache_creation",
                "cacheWrite",
            ],
        )
        .unwrap_or_default(),
        cache_read_tokens: integer_field(
            usage,
            &[
                "cache_read_input_tokens",
                "cache_read_tokens",
                "cacheReadInputTokens",
                "cacheReadTokens",
                "cache_read",
                "cacheRead",
            ],
        )
        .unwrap_or_default(),
    })
}

fn dedupe_key(value: &Value) -> Option<String> {
    let message_id = value
        .get("message")
        .and_then(|message| string_field(message, &["id"]))
        .or_else(|| string_field(value, &["messageId", "message_id"]))?;
    if let Some(request_id) = string_field(value, &["requestId", "request_id"]) {
        return Some(format!("{message_id}:{request_id}"));
    }
    Some(message_id)
}

fn extract_model(value: &Value) -> Option<String> {
    value
        .get("message")
        .and_then(|message| {
            string_field(message, &["model"]).or_else(|| {
                message
                    .get("model")
                    .and_then(|model| string_field(model, &["id", "display_name", "displayName"]))
            })
        })
        .or_else(|| string_field(value, &["model", "modelName", "model_name"]))
}

fn entries_to_daily(entries: &[UsageEntry]) -> Vec<Value> {
    let mut groups: BTreeMap<String, Aggregate> = BTreeMap::new();
    for entry in entries {
        groups
            .entry(entry.timestamp.format("%Y-%m-%d").to_string())
            .or_default()
            .add_entry(entry);
    }

    groups
        .into_iter()
        .map(|(date, group)| {
            json!({
                "date": date,
                "inputTokens": group.totals.input_tokens,
                "outputTokens": group.totals.output_tokens,
                "cacheCreationTokens": group.totals.cache_creation_tokens,
                "cacheReadTokens": group.totals.cache_read_tokens,
                "totalTokens": group.totals.total_tokens(),
                "totalCost": group.totals.total_cost,
                "modelsUsed": group.models_used(),
                "modelBreakdowns": group.model_breakdowns(),
            })
        })
        .collect()
}

fn entries_to_monthly(entries: &[UsageEntry]) -> Vec<Value> {
    let mut groups: BTreeMap<String, Aggregate> = BTreeMap::new();
    for entry in entries {
        groups
            .entry(entry.timestamp.format("%Y-%m").to_string())
            .or_default()
            .add_entry(entry);
    }

    groups
        .into_iter()
        .map(|(month, group)| {
            json!({
                "month": month,
                "inputTokens": group.totals.input_tokens,
                "outputTokens": group.totals.output_tokens,
                "cacheCreationTokens": group.totals.cache_creation_tokens,
                "cacheReadTokens": group.totals.cache_read_tokens,
                "totalTokens": group.totals.total_tokens(),
                "totalCost": group.totals.total_cost,
                "modelsUsed": group.models_used(),
                "modelBreakdowns": group.model_breakdowns(),
            })
        })
        .collect()
}

fn entries_to_sessions(entries: &[UsageEntry]) -> Vec<Value> {
    let mut groups: BTreeMap<String, (DateTime<Local>, Aggregate)> = BTreeMap::new();
    for entry in entries {
        let (last_activity, group) = groups
            .entry(entry.session_id.clone())
            .or_insert_with(|| (entry.timestamp, Aggregate::default()));
        if entry.timestamp > *last_activity {
            *last_activity = entry.timestamp;
        }
        group.add_entry(entry);
    }

    let mut rows: Vec<_> = groups
        .into_iter()
        .map(|(session_id, (last_activity, group))| {
            json!({
                "sessionId": session_id,
                "date": last_activity.format("%Y-%m-%d").to_string(),
                "time": last_activity.format("%H:%M").to_string(),
                "inputTokens": group.totals.input_tokens,
                "outputTokens": group.totals.output_tokens,
                "cacheCreationTokens": group.totals.cache_creation_tokens,
                "cacheReadTokens": group.totals.cache_read_tokens,
                "totalTokens": group.totals.total_tokens(),
                "totalCost": group.totals.total_cost,
                "modelsUsed": group.models_used(),
                "modelBreakdowns": group.model_breakdowns(),
            })
        })
        .collect();
    rows.sort_by_key(sort_key);
    rows
}

/// One row per usage-bearing message, keyed by the stable dedupe key so
/// ledger upserts stay idempotent across incremental re-scans.
fn entries_to_messages(entries: &[UsageEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| {
            json!({
                "messageId": entry.message_key,
                "sessionId": entry.session_id,
                "date": entry.timestamp.format("%Y-%m-%d").to_string(),
                "time": entry.timestamp.format("%H:%M").to_string(),
                "inputTokens": entry.usage.input_tokens,
                "outputTokens": entry.usage.output_tokens,
                "cacheCreationTokens": entry.usage.cache_creation_tokens,
                "cacheReadTokens": entry.usage.cache_read_tokens,
                "totalTokens": entry.usage.total_tokens(),
                "cost": entry.cost_usd,
            })
        })
        .collect()
}

fn entries_to_blocks(entries: &[UsageEntry]) -> Vec<Value> {
    if entries.is_empty() {
        return Vec::new();
    }

    let mut sorted_entries = entries.to_vec();
    sorted_entries.sort_by_key(|entry| entry.timestamp.timestamp_millis());

    let mut rows = Vec::new();
    let mut current_start: Option<DateTime<Local>> = None;
    let mut current_entries: Vec<UsageEntry> = Vec::new();
    let session_duration = Duration::hours(BLOCK_HOURS);

    for entry in sorted_entries {
        match current_start {
            None => {
                current_start = Some(floor_to_hour(entry.timestamp));
                current_entries.push(entry);
            }
            Some(start) => {
                let last_timestamp = current_entries
                    .last()
                    .map(|current| current.timestamp)
                    .unwrap_or(start);
                if entry.timestamp.date_naive() != start.date_naive()
                    || entry.timestamp - start > session_duration
                    || entry.timestamp - last_timestamp > session_duration
                {
                    rows.push(block_row(start, &current_entries));
                    current_start = Some(floor_to_hour(entry.timestamp));
                    current_entries.clear();
                }
                current_entries.push(entry);
            }
        }
    }

    if let Some(start) = current_start {
        if !current_entries.is_empty() {
            rows.push(block_row(start, &current_entries));
        }
    }

    rows
}

fn block_row(start: DateTime<Local>, entries: &[UsageEntry]) -> Value {
    let mut aggregate = Aggregate::default();
    let mut session_ids = BTreeSet::new();
    for entry in entries {
        aggregate.add_entry(entry);
        if !entry.session_id.is_empty() {
            session_ids.insert(entry.session_id.clone());
        }
    }

    let session_id = if session_ids.len() == 1 {
        session_ids.into_iter().next().unwrap_or_default()
    } else {
        String::new()
    };
    let model_name = aggregate
        .by_model
        .iter()
        .max_by_key(|(_, totals)| totals.total_tokens())
        .map(|(model, _)| model.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let models_used = aggregate.models_used();
    let model_breakdowns = aggregate.model_breakdowns();
    let timestamp = start.to_rfc3339();

    json!({
        "blockId": timestamp,
        "sessionId": session_id,
        "modelName": model_name,
        "modelsUsed": models_used,
        "modelBreakdowns": model_breakdowns,
        "timestamp": timestamp,
        "date": start.format("%Y-%m-%d").to_string(),
        "time": start.format("%H:%M").to_string(),
        "inputTokens": aggregate.totals.input_tokens,
        "outputTokens": aggregate.totals.output_tokens,
        "cacheCreationTokens": aggregate.totals.cache_creation_tokens,
        "cacheReadTokens": aggregate.totals.cache_read_tokens,
        "totalTokens": aggregate.totals.total_tokens(),
        "cost": aggregate.totals.total_cost,
    })
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

fn floor_to_hour(timestamp: DateTime<Local>) -> DateTime<Local> {
    let utc = timestamp.with_timezone(&Utc);
    let floored_seconds = utc.timestamp() - utc.timestamp().rem_euclid(60 * 60);
    DateTime::<Utc>::from_timestamp(floored_seconds, 0)
        .unwrap_or(utc)
        .with_timezone(&Local)
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Local>> {
    if let Some(text) = value.as_str() {
        return DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|timestamp| timestamp.with_timezone(&Local));
    }

    let number = number(value)?;
    if number.abs() > 1_000_000_000_000.0 {
        return Local.timestamp_millis_opt(number as i64).single();
    }

    let seconds = number.trunc() as i64;
    let nanos = ((number.fract().abs()) * 1_000_000_000.0).round() as u32;
    DateTime::<Utc>::from_timestamp(seconds, nanos.min(999_999_999))
        .map(|timestamp| timestamp.with_timezone(&Local))
}

fn local_day_bounds(date: &str) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let next_date = date.succ_opt()?;
    let start = Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    let end = Local
        .from_local_datetime(&next_date.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    Some((start, end))
}

fn integer_field(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(integer))
}

fn number_field(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| value.get(*key).and_then(number))
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn integer(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    if let Some(number) = value.as_f64() {
        return number.is_finite().then_some(number as i64);
    }
    value.as_str().and_then(|text| text.parse::<i64>().ok())
}

fn number(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        return number.is_finite().then_some(number);
    }
    value
        .as_str()
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|number| number.is_finite())
}

fn session_id_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn watermark_skips_unchanged_files_and_rereads_newer_ones() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let projects_dir = fixture.path.join("claude").join(PROJECTS_DIR).join("proj");
        fs::create_dir_all(&projects_dir).unwrap();
        let session_file = projects_dir.join("session-a.jsonl");
        fs::write(
            &session_file,
            r#"{"timestamp":"2024-01-01T12:15:00Z","sessionId":"s1","requestId":"r1","costUSD":0.01,"message":{"id":"m1","model":"claude-sonnet","usage":{"input_tokens":100,"output_tokens":40}}}
"#,
        )
        .unwrap();
        let mtime_ms = file_mtime_ms(&session_file);

        let previous = std::env::var_os(CLAUDE_CONFIG_DIR);
        std::env::set_var(CLAUDE_CONFIG_DIR, fixture.path.join("claude"));

        // First run (no watermark): full scan.
        let full = load_source_view_since("sessions", true, None).unwrap();
        // Unchanged file (mtime at the watermark): skipped.
        let skipped = load_source_view_since("sessions", true, Some(mtime_ms)).unwrap();
        // File newer than the watermark: re-read.
        let reread = load_source_view_since("sessions", true, Some(mtime_ms - 1)).unwrap();

        restore_env(previous);

        assert_eq!(full.len(), 1);
        assert!(skipped.is_empty());
        assert_eq!(reread.len(), 1);
    }

    #[test]
    fn loads_and_aggregates_claude_jsonl() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let projects_dir = fixture.path.join("claude").join(PROJECTS_DIR).join("proj");
        fs::create_dir_all(&projects_dir).unwrap();
        fs::write(
            projects_dir.join("session-a.jsonl"),
            r#"{"timestamp":"2024-01-01T12:15:00Z","sessionId":"s1","requestId":"r1","costUSD":0.01,"message":{"id":"m1","model":"claude-sonnet","usage":{"input_tokens":100,"output_tokens":40,"cache_creation_input_tokens":10,"cache_read_input_tokens":5}}}
not-json
{"timestamp":"2024-01-01T12:16:00Z","sessionId":"s1","requestId":"r1","costUSD":0.01,"message":{"id":"m1","model":"claude-sonnet","usage":{"input_tokens":100,"output_tokens":40}}}
{"timestamp":"2024-01-02T01:00:00Z","sessionId":"s1","requestId":"r2","costUSD":0.02,"usage":{"input":20,"output":30,"cache_creation":4,"cache_read":6},"model":"claude-opus"}
"#,
        )
        .unwrap();

        let previous = std::env::var_os(CLAUDE_CONFIG_DIR);
        std::env::set_var(CLAUDE_CONFIG_DIR, fixture.path.join("claude"));
        let daily = load_source_view("daily", false).unwrap();
        let sessions = load_source_view("sessions", false).unwrap();
        restore_env(previous);

        assert_eq!(daily.len(), 2);
        assert_eq!(daily[0]["inputTokens"], 100);
        assert_eq!(daily[0]["outputTokens"], 40);
        assert_eq!(daily[0]["cacheCreationTokens"], 10);
        assert_eq!(daily[0]["cacheReadTokens"], 5);
        assert_eq!(daily[0]["totalTokens"], 155);
        assert_eq!(daily[0]["totalCost"], 0.01);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["sessionId"], "s1");
        assert_eq!(sessions[0]["inputTokens"], 120);
        assert_eq!(sessions[0]["totalTokens"], 215);
    }

    #[test]
    fn messages_view_attributes_tokens_to_each_messages_own_hour() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let projects_dir = fixture.path.join("claude").join(PROJECTS_DIR).join("proj");
        fs::create_dir_all(&projects_dir).unwrap();
        fs::write(
            projects_dir.join("session-a.jsonl"),
            r#"{"timestamp":"2024-01-01T12:15:00Z","sessionId":"s1","requestId":"r1","costUSD":0.01,"message":{"id":"m1","model":"claude-sonnet","usage":{"input_tokens":100,"output_tokens":40}}}
{"timestamp":"2024-01-01T16:45:00Z","sessionId":"s1","requestId":"r2","costUSD":0.02,"message":{"id":"m2","model":"claude-sonnet","usage":{"input_tokens":20,"output_tokens":10}}}
"#,
        )
        .unwrap();

        let previous = std::env::var_os(CLAUDE_CONFIG_DIR);
        std::env::set_var(CLAUDE_CONFIG_DIR, fixture.path.join("claude"));
        let messages = load_source_view("messages", false).unwrap();
        let sessions = load_source_view("sessions", false).unwrap();
        restore_env(previous);

        // The sessions view collapses everything into one row keyed by the
        // session's last-activity time...
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["totalTokens"], 170);

        // ...while the messages view keeps one row per message, each with its
        // own timestamp (assertions stay timezone-agnostic on purpose).
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["messageId"], "m1:r1");
        assert_eq!(messages[0]["sessionId"], "s1");
        assert_eq!(messages[0]["totalTokens"], 140);
        assert_eq!(messages[0]["cost"], 0.01);
        assert!(messages[0]["date"].as_str().unwrap().starts_with("2024-01-"));
        assert_eq!(messages[1]["messageId"], "m2:r2");
        assert_eq!(messages[1]["totalTokens"], 30);
        assert!(
            messages[0]["time"] != messages[1]["time"]
                || messages[0]["date"] != messages[1]["date"]
        );
    }

    #[test]
    fn groups_blocks_into_five_hour_windows() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let projects_dir = fixture.path.join("claude").join(PROJECTS_DIR).join("proj");
        fs::create_dir_all(&projects_dir).unwrap();
        let first = Local
            .with_ymd_and_hms(2024, 1, 1, 12, 15, 0)
            .single()
            .unwrap();
        let second = Local
            .with_ymd_and_hms(2024, 1, 1, 16, 59, 0)
            .single()
            .unwrap();
        let third = Local
            .with_ymd_and_hms(2024, 1, 1, 18, 1, 0)
            .single()
            .unwrap();
        fs::write(
            projects_dir.join("session-a.jsonl"),
            format!(
                r#"{{"timestamp":"{}","sessionId":"s1","requestId":"r1","costUSD":0.01,"message":{{"id":"m1","model":"claude-sonnet","usage":{{"input_tokens":100,"output_tokens":40}}}}}}
{{"timestamp":"{}","sessionId":"s1","requestId":"r2","costUSD":0.02,"message":{{"id":"m2","model":"claude-sonnet","usage":{{"input_tokens":20,"output_tokens":10}}}}}}
{{"timestamp":"{}","sessionId":"s1","requestId":"r3","costUSD":0.03,"message":{{"id":"m3","model":"claude-opus","usage":{{"input_tokens":30,"output_tokens":10}}}}}}
"#,
                first.to_rfc3339(),
                second.to_rfc3339(),
                third.to_rfc3339(),
            ),
        )
        .unwrap();

        let previous = std::env::var_os(CLAUDE_CONFIG_DIR);
        std::env::set_var(CLAUDE_CONFIG_DIR, fixture.path.join("claude"));
        let blocks = load_source_view("blocks", false).unwrap();
        restore_env(previous);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["sessionId"], "s1");
        assert_eq!(blocks[0]["inputTokens"], 120);
        assert_eq!(blocks[0]["outputTokens"], 50);
        assert_eq!(blocks[1]["inputTokens"], 30);
    }

    #[test]
    fn splits_blocks_at_local_midnight() {
        let before_midnight = Local
            .with_ymd_and_hms(2026, 7, 25, 23, 50, 0)
            .single()
            .unwrap();
        let after_midnight = Local
            .with_ymd_and_hms(2026, 7, 26, 0, 10, 0)
            .single()
            .unwrap();
        let entries = vec![
            UsageEntry {
                timestamp: before_midnight,
                session_id: "s1".to_string(),
                model: "claude-opus".to_string(),
                usage: UsageCounts {
                    input_tokens: 100,
                    output_tokens: 40,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                },
                cost_usd: 0.01,
                message_key: "m1".to_string(),
            },
            UsageEntry {
                timestamp: after_midnight,
                session_id: "s1".to_string(),
                model: "claude-opus".to_string(),
                usage: UsageCounts {
                    input_tokens: 20,
                    output_tokens: 10,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                },
                cost_usd: 0.02,
                message_key: "m2".to_string(),
            },
        ];

        let blocks = entries_to_blocks(&entries);

        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[0]["date"],
            before_midnight.format("%Y-%m-%d").to_string()
        );
        assert_eq!(blocks[0]["totalTokens"], 140);
        assert_eq!(
            blocks[1]["date"],
            after_midnight.format("%Y-%m-%d").to_string()
        );
        assert_eq!(blocks[1]["totalTokens"], 30);
    }

    #[test]
    fn mixed_model_block_preserves_breakdowns_and_uses_dominant_label() {
        let start = Local
            .with_ymd_and_hms(2026, 7, 26, 10, 0, 0)
            .single()
            .unwrap();
        let entry = |minute: i64, model: &str, tokens: i64, key: &str| UsageEntry {
            timestamp: start + Duration::minutes(minute),
            session_id: "s1".to_string(),
            model: model.to_string(),
            usage: UsageCounts {
                input_tokens: tokens,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
            cost_usd: tokens as f64 / 1_000.0,
            message_key: key.to_string(),
        };
        let entries = vec![
            entry(0, "claude-haiku", 10, "m1"),
            entry(10, "claude-opus", 100, "m2"),
            entry(20, "claude-sonnet", 20, "m3"),
        ];

        let blocks = entries_to_blocks(&entries);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["modelName"], "claude-opus");
        assert_eq!(blocks[0]["totalTokens"], 130);
        let breakdowns = blocks[0]["modelBreakdowns"].as_array().unwrap();
        assert_eq!(breakdowns.len(), 3);
        let tokens_for = |model: &str| {
            breakdowns
                .iter()
                .find(|row| row["modelName"] == model)
                .map(|row| row["inputTokens"].as_i64().unwrap())
                .unwrap()
        };
        assert_eq!(tokens_for("claude-haiku"), 10);
        assert_eq!(tokens_for("claude-opus"), 100);
        assert_eq!(tokens_for("claude-sonnet"), 20);
    }

    #[test]
    fn deduplicates_cross_file_by_message_id_when_request_id_is_missing() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let projects_dir = fixture.path.join("claude").join(PROJECTS_DIR).join("proj");
        let subagents_dir = projects_dir.join("subagents");
        fs::create_dir_all(&subagents_dir).unwrap();

        // Same API call logged in two agent files (simulates parallel sub-agents)
        let event = |msg_id: &str| -> String {
            format!(
                r#"{{"timestamp":"2026-05-26T12:00:00Z","sessionId":"s1","message":{{"id":"{msg_id}","model":"deepseek-v4-flash","usage":{{"input_tokens":1000,"output_tokens":200,"cache_read_input_tokens":50000}}}}}}"#
            )
        };

        fs::write(
            subagents_dir.join("agent-a.jsonl"),
            format!("{}\n", event("m1")),
        )
        .unwrap();
        fs::write(
            subagents_dir.join("agent-b.jsonl"),
            format!("{}\n", event("m1")),
        )
        .unwrap();
        // Different API call in agent-a
        fs::write(
            subagents_dir.join("agent-a.jsonl"),
            format!("{}\n{}\n", event("m1"), event("m2")),
        )
        .unwrap();
        fs::write(
            subagents_dir.join("agent-b.jsonl"),
            format!("{}\n{}\n", event("m1"), event("m3")),
        )
        .unwrap();

        let previous = std::env::var_os(CLAUDE_CONFIG_DIR);
        std::env::set_var(CLAUDE_CONFIG_DIR, fixture.path.join("claude"));
        let daily = load_source_view("daily", false).unwrap();
        restore_env(previous);

        assert_eq!(daily.len(), 1);
        // m1 appears in both files but only counts once; m2 and m3 count once each
        assert_eq!(daily[0]["totalTokens"], json!(3 * (1000 + 200 + 50000)));
    }

    #[test]
    fn deduplicates_by_message_id_and_request_id_when_both_present() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let projects_dir = fixture.path.join("claude").join(PROJECTS_DIR).join("proj");
        fs::create_dir_all(&projects_dir).unwrap();

        // Same message_id but different request_id → different API calls, both counted
        fs::write(
            projects_dir.join("session.jsonl"),
            concat!(
                r#"{"timestamp":"2026-05-26T12:00:00Z","sessionId":"s1","requestId":"r1","message":{"id":"m1","model":"deepseek-v4-flash","usage":{"input_tokens":100,"output_tokens":10}}}"#,
                "\n",
                r#"{"timestamp":"2026-05-26T12:01:00Z","sessionId":"s1","requestId":"r2","message":{"id":"m1","model":"deepseek-v4-flash","usage":{"input_tokens":100,"output_tokens":10}}}"#,
                "\n",
            ),
        )
        .unwrap();

        let previous = std::env::var_os(CLAUDE_CONFIG_DIR);
        std::env::set_var(CLAUDE_CONFIG_DIR, fixture.path.join("claude"));
        let daily = load_source_view("daily", false).unwrap();
        restore_env(previous);

        assert_eq!(daily.len(), 1);
        // Both events counted because request_id differs
        assert_eq!(daily[0]["totalTokens"], json!(2 * (100 + 10)));
    }

    fn restore_env(previous: Option<std::ffi::OsString>) {
        if let Some(value) = previous {
            std::env::set_var(CLAUDE_CONFIG_DIR, value);
        } else {
            std::env::remove_var(CLAUDE_CONFIG_DIR);
        }
    }

    fn file_mtime_ms(path: &Path) -> i64 {
        let modified = fs::metadata(path).unwrap().modified().unwrap();
        i64::try_from(modified.duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap()
    }

    struct TestFixture {
        path: PathBuf,
    }

    impl TestFixture {
        fn new() -> Self {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("token-usage-claude-{}-{now}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
