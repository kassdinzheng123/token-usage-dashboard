use crate::{
    pricing::{model_cost_usd, TokenUsage},
    sources::{home_dir, to_i64},
};
use chrono::{DateTime, Local, TimeZone};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    convert::TryFrom,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const FACTORY_DIR_ENV: &str = "FACTORY_DIR";
const FACTORY_SESSIONS_DIR_ENV: &str = "FACTORY_SESSIONS_DIR";
const UNKNOWN_MODEL: &str = "unknown";

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

#[derive(Debug, Clone, Default)]
struct SessionMetadata {
    session_id: String,
    timestamp_millis: Option<i64>,
    title: Option<String>,
    cwd: Option<String>,
}

#[derive(Debug, Clone)]
struct FactorySession {
    session_id: String,
    title: Option<String>,
    cwd: Option<String>,
    timestamp: DateTime<Local>,
    model_name: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_cost: f64,
}

impl FactorySession {
    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Clone, Default)]
struct Totals {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_cost: f64,
}

impl Totals {
    fn add_session(&mut self, session: &FactorySession) {
        self.input_tokens += session.input_tokens;
        self.output_tokens += session.output_tokens;
        self.cache_creation_tokens += session.cache_creation_tokens;
        self.cache_read_tokens += session.cache_read_tokens;
        self.total_cost += session.total_cost;
    }

    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Clone, Default)]
struct Aggregate {
    totals: Totals,
    by_model: BTreeMap<String, Totals>,
}

impl Aggregate {
    fn add_session(&mut self, session: &FactorySession) {
        self.totals.add_session(session);
        self.by_model
            .entry(session.model_name.clone())
            .or_default()
            .add_session(session);
    }

    fn models_used(&self) -> Vec<String> {
        self.by_model.keys().cloned().collect()
    }

    fn model_breakdowns(&self) -> Vec<Value> {
        self.by_model
            .iter()
            .map(|(model, totals)| model_breakdown(model, totals))
            .collect()
    }
}

pub fn load_source_view(view: &str, refresh: bool) -> Result<Vec<Value>, String> {
    let _ = refresh;
    let view = SourceView::try_from(view)?;

    if view == SourceView::Blocks {
        return Ok(Vec::new());
    }

    let sessions = load_sessions()?;
    Ok(match view {
        SourceView::Daily => sessions_to_daily(&sessions),
        SourceView::Monthly => sessions_to_monthly(&sessions),
        SourceView::Sessions => sessions_to_sessions(&sessions),
        SourceView::Blocks => Vec::new(),
    })
}

fn load_sessions() -> Result<Vec<FactorySession>, String> {
    let Some(sessions_dir) = discover_sessions_dir() else {
        return Ok(Vec::new());
    };

    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }

    let metadata = load_index_metadata(&sessions_dir)?;
    let mut files = Vec::new();
    collect_settings_files(&sessions_dir, &mut files)?;
    files.sort();

    let mut sessions = Vec::new();
    for path in files {
        let session_id = settings_session_id(&path);
        let indexed = metadata.get(&session_id);
        let jsonl_metadata = read_jsonl_metadata(&path);
        if let Some(session) = parse_settings_file(
            &path,
            &session_id,
            indexed,
            jsonl_metadata.as_ref(),
            file_modified_millis(&path),
        )? {
            sessions.push(session);
        }
    }

    sessions.sort_by_key(|session| session.timestamp.timestamp_millis());
    Ok(sessions)
}

fn discover_sessions_dir() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(FACTORY_SESSIONS_DIR_ENV) {
        let path = PathBuf::from(raw);
        if path.is_dir() {
            return Some(path);
        }
    }

    if let Some(raw) = std::env::var_os(FACTORY_DIR_ENV) {
        let path = PathBuf::from(raw).join("sessions");
        if path.is_dir() {
            return Some(path);
        }
    }

    home_dir().map(|home| home.join(".factory").join("sessions"))
}

fn load_index_metadata(sessions_dir: &Path) -> Result<BTreeMap<String, SessionMetadata>, String> {
    let Some(factory_dir) = sessions_dir.parent() else {
        return Ok(BTreeMap::new());
    };
    let index_path = factory_dir.join("sessions-index.json");
    let contents = match fs::read_to_string(&index_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => return Err(format!("failed to read {}: {err}", index_path.display())),
    };
    let value = serde_json::from_str::<Value>(&contents)
        .map_err(|err| format!("failed to parse {}: {err}", index_path.display()))?;
    let Some(entries) = value.get("entries").and_then(Value::as_array) else {
        return Ok(BTreeMap::new());
    };

    let mut metadata = BTreeMap::new();
    for entry in entries {
        let Some(session_id) = entry.get("sessionId").and_then(Value::as_str) else {
            continue;
        };
        metadata.insert(
            session_id.to_owned(),
            SessionMetadata {
                session_id: session_id.to_owned(),
                timestamp_millis: entry
                    .get("settingsMtime")
                    .and_then(as_i64)
                    .or_else(|| entry.get("mtime").and_then(as_i64)),
                title: string_field(entry, "title"),
                cwd: string_field(entry, "cwd"),
            },
        );
    }

    Ok(metadata)
}

fn collect_settings_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
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
            collect_settings_files(&path, files)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".settings.json"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn parse_settings_file(
    path: &Path,
    session_id: &str,
    indexed: Option<&SessionMetadata>,
    jsonl_metadata: Option<&SessionMetadata>,
    modified_millis: Option<i64>,
) -> Result<Option<FactorySession>, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
    };
    let value = match serde_json::from_str::<Value>(&contents) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    Ok(parse_settings_value(
        &value,
        session_id,
        indexed,
        jsonl_metadata,
        modified_millis,
    ))
}

fn parse_settings_value(
    value: &Value,
    session_id: &str,
    indexed: Option<&SessionMetadata>,
    jsonl_metadata: Option<&SessionMetadata>,
    modified_millis: Option<i64>,
) -> Option<FactorySession> {
    let usage = value.get("tokenUsage")?.as_object()?;
    let input_tokens = usage.get("inputTokens").map(to_i64).unwrap_or_default();
    let output_tokens = usage.get("outputTokens").map(to_i64).unwrap_or_default()
        + usage.get("thinkingTokens").map(to_i64).unwrap_or_default();
    let cache_creation_tokens = usage
        .get("cacheCreationTokens")
        .map(to_i64)
        .unwrap_or_default();
    let cache_read_tokens = usage.get("cacheReadTokens").map(to_i64).unwrap_or_default();
    let total_tokens = input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens;
    if total_tokens <= 0 {
        return None;
    }

    let model_name = value
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| value.get("specModeModel").and_then(Value::as_str))
        .filter(|model| !model.trim().is_empty())
        .unwrap_or(UNKNOWN_MODEL)
        .to_owned();
    let total_cost = model_cost_usd(
        &model_name,
        TokenUsage {
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        },
    );
    let timestamp_millis = indexed
        .and_then(|metadata| metadata.timestamp_millis)
        .or_else(|| jsonl_metadata.and_then(|metadata| metadata.timestamp_millis))
        .or(modified_millis)?;
    let timestamp = Local.timestamp_millis_opt(timestamp_millis).single()?;

    Some(FactorySession {
        session_id: indexed
            .map(|metadata| metadata.session_id.clone())
            .unwrap_or_else(|| session_id.to_owned()),
        title: indexed
            .and_then(|metadata| metadata.title.clone())
            .or_else(|| jsonl_metadata.and_then(|metadata| metadata.title.clone())),
        cwd: indexed
            .and_then(|metadata| metadata.cwd.clone())
            .or_else(|| jsonl_metadata.and_then(|metadata| metadata.cwd.clone())),
        timestamp,
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_cost,
    })
}

fn read_jsonl_metadata(settings_path: &Path) -> Option<SessionMetadata> {
    let jsonl_path =
        settings_path.with_file_name(format!("{}.jsonl", settings_session_id(settings_path)));
    let file = File::open(jsonl_path).ok()?;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = serde_json::from_str::<Value>(trimmed).ok()?;
        if value.get("type").and_then(Value::as_str) == Some("session_start") {
            return Some(SessionMetadata {
                session_id: value
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                timestamp_millis: None,
                title: value
                    .get("sessionTitle")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("title").and_then(Value::as_str))
                    .filter(|title| !title.trim().is_empty())
                    .map(ToOwned::to_owned),
                cwd: string_field(&value, "cwd"),
            });
        }
        if let Some(timestamp) = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp_millis)
        {
            return Some(SessionMetadata {
                session_id: String::new(),
                timestamp_millis: Some(timestamp),
                title: None,
                cwd: None,
            });
        }
    }

    None
}

fn sessions_to_daily(sessions: &[FactorySession]) -> Vec<Value> {
    aggregate_by(sessions, |session| {
        session.timestamp.format("%Y-%m-%d").to_string()
    })
    .into_iter()
    .map(|(date, aggregate)| usage_row("date", date, aggregate))
    .collect()
}

fn sessions_to_monthly(sessions: &[FactorySession]) -> Vec<Value> {
    aggregate_by(sessions, |session| {
        session.timestamp.format("%Y-%m").to_string()
    })
    .into_iter()
    .map(|(month, aggregate)| usage_row("month", month, aggregate))
    .collect()
}

fn sessions_to_sessions(sessions: &[FactorySession]) -> Vec<Value> {
    sessions
        .iter()
        .map(|session| {
            let mut totals = Totals::default();
            totals.add_session(session);
            json!({
                "sessionId": session.session_id.clone(),
                "date": session.timestamp.format("%Y-%m-%d").to_string(),
                "time": session.timestamp.format("%H:%M").to_string(),
                "inputTokens": session.input_tokens,
                "outputTokens": session.output_tokens,
                "cacheCreationTokens": session.cache_creation_tokens,
                "cacheReadTokens": session.cache_read_tokens,
                "totalTokens": session.total_tokens(),
                "totalCost": session.total_cost,
                "modelsUsed": [session.model_name.clone()],
                "modelBreakdowns": [model_breakdown(&session.model_name, &totals)],
                "title": session.title.clone(),
                "projectPath": session.cwd.clone(),
            })
        })
        .collect()
}

fn aggregate_by(
    sessions: &[FactorySession],
    key_for: impl Fn(&FactorySession) -> String,
) -> BTreeMap<String, Aggregate> {
    let mut grouped = BTreeMap::new();
    for session in sessions {
        grouped
            .entry(key_for(session))
            .or_insert_with(Aggregate::default)
            .add_session(session);
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

fn model_breakdown(model_name: &str, totals: &Totals) -> Value {
    json!({
        "modelName": model_name,
        "inputTokens": totals.input_tokens,
        "outputTokens": totals.output_tokens,
        "cacheCreationTokens": totals.cache_creation_tokens,
        "cacheReadTokens": totals.cache_read_tokens,
        "cost": totals.total_cost,
    })
}

fn settings_session_id(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".settings.json"))
        .unwrap_or("unknown")
        .to_owned()
}

fn file_modified_millis(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_millis()).ok()
}

fn parse_timestamp_millis(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date_time| date_time.timestamp_millis())
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn as_i64(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    value.as_f64().and_then(|number| {
        if number.is_finite() {
            Some(number as i64)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_settings_value_maps_thinking_tokens_to_output() {
        let metadata = SessionMetadata {
            session_id: "session-1".to_owned(),
            timestamp_millis: Some(1_704_164_645_000),
            title: Some("Build feature".to_owned()),
            cwd: Some("/repo".to_owned()),
        };

        let session = parse_settings_value(
            &json!({
                "model": "custom:factory-model",
                "tokenUsage": {
                    "inputTokens": 100,
                    "outputTokens": 40,
                    "cacheCreationTokens": 5,
                    "cacheReadTokens": 7,
                    "thinkingTokens": 9
                }
            }),
            "fallback",
            Some(&metadata),
            None,
            None,
        )
        .unwrap();

        assert_eq!(session.session_id, "session-1");
        assert_eq!(session.title.as_deref(), Some("Build feature"));
        assert_eq!(session.cwd.as_deref(), Some("/repo"));
        assert_eq!(session.model_name, "custom:factory-model");
        assert_eq!(session.input_tokens, 100);
        assert_eq!(session.output_tokens, 49);
        assert_eq!(session.cache_creation_tokens, 5);
        assert_eq!(session.cache_read_tokens, 7);
        assert_eq!(session.total_tokens(), 161);
    }

    #[test]
    fn daily_and_monthly_aggregate_by_session_timestamp() {
        let first = parse_settings_value(
            &json!({
                "model": "mimo-v2.5-pro",
                "tokenUsage": { "inputTokens": 10, "outputTokens": 2 }
            }),
            "session-1",
            None,
            None,
            Some(1_704_164_645_000),
        )
        .unwrap();
        let second = parse_settings_value(
            &json!({
                "model": "mimo-v2.5-pro",
                "tokenUsage": { "inputTokens": 5, "outputTokens": 3, "cacheReadTokens": 4 }
            }),
            "session-2",
            None,
            None,
            Some(1_704_165_000_000),
        )
        .unwrap();

        let daily = sessions_to_daily(&[first.clone(), second.clone()]);
        let monthly = sessions_to_monthly(&[first, second]);

        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0]["inputTokens"], json!(15));
        assert_eq!(daily[0]["outputTokens"], json!(5));
        assert_eq!(daily[0]["cacheReadTokens"], json!(4));
        assert_eq!(daily[0]["totalTokens"], json!(24));
        let daily_cost = daily[0]["totalCost"].as_f64().unwrap_or_default();
        let expected_cost = (15.0 * 1.0 + 5.0 * 3.0 + 4.0 * 0.2) / 1_000_000.0;
        assert!((daily_cost - expected_cost).abs() < 1e-12);
        assert_eq!(daily[0]["modelBreakdowns"][0]["cost"], json!(expected_cost));
        assert_eq!(monthly.len(), 1);
        assert_eq!(monthly[0]["modelsUsed"], json!(["mimo-v2.5-pro"]));
    }
}
