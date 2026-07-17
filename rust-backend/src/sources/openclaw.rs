use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, num, to_i64, unix_millis_to_utc_parts, LocalSession, SourceError};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn load_source_view(view: &str, refresh: bool) -> Result<Vec<Value>, String> {
    load_source_view_since(view, refresh, None)
}

pub fn load_source_view_since(
    view: &str,
    _refresh: bool,
    watermark_ms: Option<i64>,
) -> Result<Vec<Value>, String> {
    let sessions = load_sessions(watermark_ms).map_err(|err| err.to_string())?;
    Ok(match view {
        "daily" => sessions_to_daily(&sessions),
        "monthly" => sessions_to_monthly(&sessions),
        "sessions" => sessions_to_sessions(&sessions),
        "messages" => sessions_to_messages(&sessions),
        "blocks" => Vec::new(),
        other => return Err(format!("unsupported view: {other}")),
    })
}

/// Per-message rows: unlike `sessions_to_sessions`, this keeps the per-line
/// events unaggregated so hourly buckets reflect when each message happened.
fn sessions_to_messages(sessions: &[LocalSession]) -> Vec<Value> {
    sessions
        .iter()
        .map(|session| {
            json!({
                "messageId": session.session_id,
                "sessionId": session_base_id(&session.session_id),
                "date": session.date,
                "time": session.time,
                "inputTokens": session.input_tokens,
                "outputTokens": session.output_tokens,
                "cacheCreationTokens": session.cache_creation_tokens,
                "cacheReadTokens": session.cache_read_tokens,
                "totalTokens": session.total_tokens(),
                "cost": session.total_cost,
            })
        })
        .collect()
}

pub fn load_sessions(watermark_ms: Option<i64>) -> Result<Vec<LocalSession>, SourceError> {
    let Some(agents_dir) = home_dir().map(|home| home.join(".openclaw").join("agents")) else {
        return Ok(Vec::new());
    };

    let Ok(agent_entries) = fs::read_dir(agents_dir) else {
        return Ok(Vec::new());
    };

    let mut sessions = Vec::new();
    for agent_entry in agent_entries {
        let Ok(agent_entry) = agent_entry else {
            continue;
        };
        let sessions_dir = agent_entry.path().join("sessions");
        let sessions_path = sessions_dir.join("sessions.json");
        // Summary-only entries are skipped when the index file itself is old.
        let index_modified = watermark_ms
            .map_or(true, |watermark| super::file_modified_after(&sessions_path, watermark));
        let Ok(contents) = fs::read_to_string(&sessions_path) else {
            continue;
        };
        let Ok(data) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        append_sessions_from_value(&mut sessions, &data, &sessions_dir, watermark_ms, index_modified);
    }

    Ok(sessions)
}

fn append_sessions_from_value(
    sessions: &mut Vec<LocalSession>,
    data: &Value,
    sessions_dir: &Path,
    watermark_ms: Option<i64>,
    index_modified: bool,
) {
    let Some(entries) = data.as_object() else {
        return;
    };

    for (key, raw) in entries {
        let Some(entry) = raw.as_object() else {
            continue;
        };

        let fallback_session_id =
            string_field(entry.get("sessionId")).unwrap_or_else(|| key.clone());
        let fallback_model =
            string_field(entry.get("model")).unwrap_or_else(|| "unknown".to_string());

        if let Some(session_file) = string_field(entry.get("sessionFile")) {
            let session_path = resolve_session_file(sessions_dir, &session_file);
            if let Some(watermark) = watermark_ms {
                if !super::file_modified_after(&session_path, watermark) {
                    // Skip entirely: falling back to the summary row would
                    // double-count against the ledger's detailed rows (their
                    // session ids differ).
                    continue;
                }
            }
            let added = append_message_usage_from_file(
                sessions,
                &session_path,
                &fallback_session_id,
                &fallback_model,
            );
            if added > 0 {
                continue;
            }
        } else if !index_modified {
            continue;
        }

        if let Some(session) = session_from_summary(key, entry) {
            sessions.push(session);
        }
    }
}

fn append_message_usage_from_file(
    sessions: &mut Vec<LocalSession>,
    session_path: &Path,
    fallback_session_id: &str,
    fallback_model: &str,
) -> usize {
    let Ok(contents) = fs::read_to_string(session_path) else {
        return 0;
    };

    let mut added = 0;
    for (index, line) in contents.lines().enumerate() {
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(session) = session_from_message_event(
            &raw,
            fallback_session_id,
            fallback_model,
            index.saturating_add(1),
        ) else {
            continue;
        };
        sessions.push(session);
        added += 1;
    }
    added
}

fn session_from_message_event(
    raw: &Value,
    fallback_session_id: &str,
    fallback_model: &str,
    line_number: usize,
) -> Option<LocalSession> {
    if raw.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }

    let message = raw.get("message").and_then(Value::as_object)?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }

    let usage = message.get("usage").and_then(Value::as_object)?;
    let timestamp =
        parse_timestamp_millis(raw.get("timestamp").or_else(|| message.get("timestamp"))?)?;
    let (date, time) = unix_millis_to_utc_parts(timestamp)?;
    let model_name = string_field(message.get("model"))
        .or_else(|| string_field(raw.get("model")))
        .or_else(|| string_field(raw.get("modelId")))
        .unwrap_or_else(|| fallback_model.to_string());

    // OpenClaw (Codex-shaped) reports input inclusive of cacheRead. Strip it so
    // pricing does not bill the cached slice at both input and cache-read rates.
    let raw_input_tokens = usage_i64(usage.get("input").or_else(|| usage.get("inputTokens")));
    let output_tokens = usage_i64(usage.get("output").or_else(|| usage.get("outputTokens")));
    let cache_creation_tokens = usage_i64(
        usage
            .get("cacheWrite")
            .or_else(|| usage.get("cacheCreationTokens"))
            .or_else(|| usage.get("cache_creation_input_tokens")),
    );
    let cache_read_tokens = usage_i64(
        usage
            .get("cacheRead")
            .or_else(|| usage.get("cacheReadTokens"))
            .or_else(|| usage.get("cache_read_input_tokens")),
    );
    let input_tokens = raw_input_tokens.saturating_sub(cache_read_tokens);
    // Drop payload totals — they follow inclusive-input arithmetic.
    let total_tokens_override = None;
    let total_cost = event_cost(usage);

    let session = LocalSession {
        session_id: format!("{fallback_session_id}#{line_number}"),
        date,
        time,
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_tokens_override,
        total_cost,
    };

    (session.total_tokens() > 0 || session.total_cost > 0.0).then_some(session)
}

fn session_from_summary(key: &str, entry: &serde_json::Map<String, Value>) -> Option<LocalSession> {
    let timestamp = entry.get("updatedAt").map(to_i64).unwrap_or_default();
    let (date, time) = unix_millis_to_utc_parts(timestamp)?;

    let session_id = string_field(entry.get("sessionId")).unwrap_or_else(|| key.to_string());
    let model_name = string_field(entry.get("model")).unwrap_or_else(|| "unknown".to_string());
    let raw_input_tokens = summary_i64(entry, &["inputTokens", "input"]);
    let output_tokens = summary_i64(entry, &["outputTokens", "output"]);
    let cache_creation_tokens = summary_i64(entry, &["cacheWrite", "cacheCreationTokens"]);
    let cache_read_tokens = summary_i64(entry, &["cacheRead", "cacheReadTokens"]);
    let input_tokens = raw_input_tokens.saturating_sub(cache_read_tokens);
    let total_tokens_override = None;
    let stored_cost = entry
        .get("costUSD")
        .map(num)
        .filter(|cost| *cost > 0.0)
        .or_else(|| entry.get("totalCost").map(num))
        .unwrap_or_default();
    let total_cost = if stored_cost > 0.0 {
        stored_cost
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

    let session = LocalSession {
        session_id,
        date,
        time,
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_tokens_override,
        total_cost,
    };

    (session.total_tokens() > 0 || session.total_cost > 0.0).then_some(session)
}

fn sessions_to_daily(sessions: &[LocalSession]) -> Vec<Value> {
    aggregate_sessions(
        sessions,
        |session| session.date.clone(),
        |group| {
            json!({
                "date": group.key,
                "inputTokens": group.input_tokens,
                "outputTokens": group.output_tokens,
                "cacheCreationTokens": group.cache_creation_tokens,
                "cacheReadTokens": group.cache_read_tokens,
                "totalTokens": group.total_tokens,
                "totalCost": group.total_cost,
                "modelsUsed": group.models_used,
                "modelBreakdowns": group.model_breakdowns,
            })
        },
    )
}

fn sessions_to_monthly(sessions: &[LocalSession]) -> Vec<Value> {
    aggregate_sessions(
        sessions,
        |session| session.date.chars().take(7).collect(),
        |group| {
            json!({
                "month": group.key,
                "inputTokens": group.input_tokens,
                "outputTokens": group.output_tokens,
                "cacheCreationTokens": group.cache_creation_tokens,
                "cacheReadTokens": group.cache_read_tokens,
                "totalTokens": group.total_tokens,
                "totalCost": group.total_cost,
                "modelsUsed": group.models_used,
                "modelBreakdowns": group.model_breakdowns,
            })
        },
    )
}

fn sessions_to_sessions(sessions: &[LocalSession]) -> Vec<Value> {
    aggregate_sessions(
        sessions,
        |session| session_base_id(&session.session_id),
        |group| {
            json!({
                "sessionId": group.key,
                "date": group.latest_date,
                "time": group.latest_time,
                "inputTokens": group.input_tokens,
                "outputTokens": group.output_tokens,
                "cacheCreationTokens": group.cache_creation_tokens,
                "cacheReadTokens": group.cache_read_tokens,
                "totalTokens": group.total_tokens,
                "totalCost": group.total_cost,
                "modelsUsed": group.models_used,
                "modelBreakdowns": group.model_breakdowns,
            })
        },
    )
}

fn aggregate_sessions(
    sessions: &[LocalSession],
    key_for: impl Fn(&LocalSession) -> String,
    row_for: impl Fn(OpenClawAggregate) -> Value,
) -> Vec<Value> {
    let mut groups: BTreeMap<String, OpenClawAggregate> = BTreeMap::new();

    for session in sessions {
        let key = key_for(session);
        let group = groups
            .entry(key.clone())
            .or_insert_with(|| OpenClawAggregate {
                key,
                ..OpenClawAggregate::default()
            });

        group.input_tokens += session.input_tokens;
        group.output_tokens += session.output_tokens;
        group.cache_creation_tokens += session.cache_creation_tokens;
        group.cache_read_tokens += session.cache_read_tokens;
        group.total_tokens += session.total_tokens();
        group.total_cost += session.total_cost;
        if group.latest_date.is_empty()
            || format!("{}T{}", session.date, session.time)
                >= format!("{}T{}", group.latest_date, group.latest_time)
        {
            group.latest_date = session.date.clone();
            group.latest_time = session.time.clone();
        }

        let clustered = super::cluster_model_name(&session.model_name);
        if !group.models_used.contains(&clustered) {
            group.models_used.push(clustered);
        }
        group.model_breakdowns.push(model_breakdown(session));
    }

    groups.into_values().map(row_for).collect()
}

#[derive(Default)]
struct OpenClawAggregate {
    key: String,
    latest_date: String,
    latest_time: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    total_cost: f64,
    models_used: Vec<String>,
    model_breakdowns: Vec<Value>,
}

fn model_breakdown(session: &LocalSession) -> Value {
    json!({
        "modelName": super::cluster_model_name(&session.model_name),
        "inputTokens": session.input_tokens,
        "outputTokens": session.output_tokens,
        "cacheCreationTokens": session.cache_creation_tokens,
        "cacheReadTokens": session.cache_read_tokens,
        "cost": session.total_cost,
    })
}

fn resolve_session_file(sessions_dir: &Path, session_file: &str) -> PathBuf {
    let path = PathBuf::from(session_file);
    if path.is_absolute() {
        path
    } else {
        sessions_dir.join(path)
    }
}

fn parse_timestamp_millis(value: &Value) -> Option<i64> {
    if let Some(text) = value.as_str() {
        if let Ok(number) = text.parse::<i64>() {
            return normalize_epoch_millis(number);
        }
        return chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|date| date.timestamp_millis());
    }

    if let Some(number) = value.as_i64() {
        return normalize_epoch_millis(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok().and_then(normalize_epoch_millis);
    }
    if let Some(number) = value.as_f64() {
        if !number.is_finite() || number <= 0.0 {
            return None;
        }
        return normalize_epoch_millis(number as i64);
    }
    None
}

fn normalize_epoch_millis(value: i64) -> Option<i64> {
    if value <= 0 {
        return None;
    }
    if value < 10_000_000_000 {
        value.checked_mul(1000)
    } else {
        Some(value)
    }
}

fn event_cost(usage: &serde_json::Map<String, Value>) -> f64 {
    usage
        .get("cost")
        .and_then(|cost| {
            cost.as_object()
                .and_then(|cost| cost.get("total").map(num))
                .or_else(|| Some(num(cost)))
        })
        .filter(|cost| *cost > 0.0)
        .or_else(|| usage.get("costUSD").map(num).filter(|cost| *cost > 0.0))
        .or_else(|| usage.get("totalCost").map(num))
        .unwrap_or_default()
}

fn summary_i64(entry: &serde_json::Map<String, Value>, keys: &[&str]) -> i64 {
    usage_i64(keys.iter().find_map(|key| entry.get(*key)))
}

fn usage_i64(value: Option<&Value>) -> i64 {
    value.map(to_i64).unwrap_or_default().max(0)
}

fn session_base_id(session_id: &str) -> String {
    session_id
        .split_once('#')
        .map(|(base, _)| base)
        .unwrap_or(session_id)
        .to_string()
}

fn string_field(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn daily_and_monthly_use_message_timestamp_not_updated_at() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let sessions_dir = fixture.path.join(".openclaw/agents/main/sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::write(
            sessions_dir.join("sessions.json"),
            r#"{
                "agent:main:main": {
                    "sessionId": "session-1",
                    "model": "gpt-5.4",
                    "updatedAt": 1706745600000,
                    "sessionFile": "session-1.jsonl",
                    "inputTokens": 999,
                    "outputTokens": 999,
                    "cacheRead": 999
                }
            }"#,
        )
        .unwrap();
        fs::write(
            sessions_dir.join("session-1.jsonl"),
            r#"{"type":"message","timestamp":"2024-01-15T12:00:00Z","message":{"role":"assistant","usage":{"input":10,"output":2,"cacheRead":3,"cacheWrite":4,"cost":{"total":0.25}}}}
{"type":"message","timestamp":"2024-02-15T12:00:00Z","message":{"role":"assistant","usage":{"input":5,"output":1,"cacheRead":2,"cacheWrite":0}}}
"#,
        )
        .unwrap();

        with_home(&fixture.path, || {
            let daily = load_source_view("daily", false).unwrap();
            assert_eq!(daily.len(), 2);
            assert_eq!(daily[0]["date"], "2024-01-15");
            // input is exclusive of cacheRead: 10 - 3 = 7
            assert_eq!(daily[0]["inputTokens"], 7);
            assert_eq!(daily[0]["cacheReadTokens"], 3);
            assert_eq!(daily[0]["cacheCreationTokens"], 4);
            assert_eq!(daily[0]["totalTokens"], 16);
            assert_eq!(daily[0]["totalCost"], 0.25);
            assert_eq!(daily[1]["date"], "2024-02-15");
            // input exclusive: 5 - 2 = 3; total = 3+1+0+2 = 6
            assert_eq!(daily[1]["inputTokens"], 3);
            assert_eq!(daily[1]["totalTokens"], 6);

            let monthly = load_source_view("monthly", false).unwrap();
            assert_eq!(monthly.len(), 2);
            assert_eq!(monthly[0]["month"], "2024-01");
            assert_eq!(monthly[0]["totalTokens"], 16);
            assert_eq!(monthly[1]["month"], "2024-02");
            assert_eq!(monthly[1]["totalTokens"], 6);
        });
    }

    #[test]
    fn sessions_view_rolls_up_message_events_by_session() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let sessions_dir = fixture.path.join(".openclaw/agents/main/sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::write(
            sessions_dir.join("sessions.json"),
            r#"{
                "agent:main:main": {
                    "sessionId": "session-1",
                    "model": "gpt-5.4",
                    "updatedAt": 1706745600000,
                    "sessionFile": "session-1.jsonl"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            sessions_dir.join("session-1.jsonl"),
            r#"{"type":"message","timestamp":"2024-01-15T12:00:00Z","message":{"role":"assistant","usage":{"input":10,"output":2,"cacheRead":3,"cacheWrite":4}}}
{"type":"message","timestamp":"2024-02-15T12:00:00Z","message":{"role":"assistant","usage":{"input":5,"output":1,"cacheRead":2,"cacheWrite":0}}}
"#,
        )
        .unwrap();

        with_home(&fixture.path, || {
            let sessions = load_source_view("sessions", false).unwrap();
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0]["sessionId"], "session-1");
            assert_eq!(sessions[0]["date"], "2024-02-15");
            // exclusive inputs: (10-3)+(5-2)=10
            assert_eq!(sessions[0]["inputTokens"], 10);
            assert_eq!(sessions[0]["outputTokens"], 3);
            assert_eq!(sessions[0]["cacheCreationTokens"], 4);
            assert_eq!(sessions[0]["cacheReadTokens"], 5);
            assert_eq!(sessions[0]["totalTokens"], 22);
        });
    }

    #[test]
    fn summary_fallback_is_used_when_session_file_is_missing() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        let sessions_dir = fixture.path.join(".openclaw/agents/main/sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::write(
            sessions_dir.join("sessions.json"),
            r#"{
                "agent:main:main": {
                    "sessionId": "session-1",
                    "model": "gpt-5.4",
                    "updatedAt": 1706745600000,
                    "inputTokens": 10,
                    "outputTokens": 2,
                    "cacheRead": 3,
                    "cacheWrite": 4
                }
            }"#,
        )
        .unwrap();

        with_home(&fixture.path, || {
            let daily = load_source_view("daily", false).unwrap();
            assert_eq!(daily.len(), 1);
            assert_eq!(daily[0]["date"], "2024-02-01");
            assert_eq!(daily[0]["inputTokens"], 7);
            assert_eq!(daily[0]["totalTokens"], 16);
        });
    }

    fn with_home(root: &Path, test: impl FnOnce()) {
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", root);
        test();
        if let Some(previous) = previous {
            std::env::set_var("HOME", previous);
        } else {
            std::env::remove_var("HOME");
        }
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
                .join(format!("token-usage-openclaw-{}-{now}", std::process::id()));
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
