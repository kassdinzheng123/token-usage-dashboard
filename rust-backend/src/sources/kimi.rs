use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{
    home_dir, iso8601_to_local_parts, to_i64, unix_millis_to_utc_parts, LocalModelUsage,
    LocalSession, SourceError,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const KIMI_CODE_HOME_ENV: &str = "KIMI_CODE_HOME";
const KIMI_WORK_HOME_ENV: &str = "KIMI_WORK_HOME";
const KIMI_WORK_RELATIVE_HOME: &str =
    "Library/Application Support/kimi-desktop/daimon-share/daimon/runtime/kimi-code/home";
const UNKNOWN_MODEL: &str = "unknown";
const SECONDARY_MODEL_ALIAS: &str = "__secondary__";

struct KimiRoot {
    prefix: &'static str,
    path: PathBuf,
}

struct KimiUsageRecord {
    agent_name: String,
    line_number: usize,
    time_ms: Option<i64>,
    model_name: Option<String>,
    usage: TokenUsage,
}

struct ParsedKimiSession {
    model_name: String,
    first_time_ms: Option<i64>,
    usage_records: Vec<KimiUsageRecord>,
}

pub fn load_sessions(watermark_ms: Option<i64>) -> Result<Vec<LocalSession>, SourceError> {
    let mut sessions = Vec::new();
    for root in kimi_roots() {
        for dir in session_dirs(&root.path) {
            if let Some(watermark) = watermark_ms {
                if !session_modified_after(&dir, watermark) {
                    continue;
                }
            }
            if let Some(session) = load_session(root.prefix, &dir) {
                sessions.push(session);
            }
        }
    }
    Ok(sessions)
}

/// True when any `agents/*/wire.jsonl` under the session dir changed after the
/// watermark. Fails open (re-scan) when the agents dir cannot be listed.
fn session_modified_after(dir: &Path, watermark_ms: i64) -> bool {
    let Ok(agents) = fs::read_dir(dir.join("agents")) else {
        return true;
    };
    for agent in agents.flatten() {
        if super::file_modified_after(&agent.path().join("wire.jsonl"), watermark_ms) {
            return true;
        }
    }
    false
}

fn kimi_roots() -> Vec<KimiRoot> {
    let mut roots = Vec::new();

    let code_home = std::env::var_os(KIMI_CODE_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".kimi-code")));
    if let Some(path) = code_home.filter(|path| path.is_dir()) {
        roots.push(KimiRoot {
            prefix: "kimi-code",
            path,
        });
    }

    let work_home = std::env::var_os(KIMI_WORK_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(KIMI_WORK_RELATIVE_HOME)));
    if let Some(path) = work_home.filter(|path| path.is_dir()) {
        roots.push(KimiRoot {
            prefix: "kimi-work",
            path,
        });
    }

    roots
}

fn session_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Ok(workspaces) = fs::read_dir(root.join("sessions")) else {
        return dirs;
    };
    for workspace in workspaces.flatten() {
        let workspace_path = workspace.path();
        if !workspace_path.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&workspace_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
    }
    dirs
}

fn load_session(prefix: &str, dir: &Path) -> Option<LocalSession> {
    let session_id = dir.file_name().and_then(|name| name.to_str())?;
    let parsed = parse_session(dir)?;
    let mut usage = TokenUsage::default();
    let mut total_cost = 0.0;
    let mut model_breakdowns: BTreeMap<String, LocalModelUsage> = BTreeMap::new();
    for record in &parsed.usage_records {
        let record_total_tokens = record.usage.input_tokens
            + record.usage.output_tokens
            + record.usage.cache_creation_tokens
            + record.usage.cache_read_tokens;
        if record_total_tokens <= 0 {
            continue;
        }
        usage.input_tokens += record.usage.input_tokens;
        usage.output_tokens += record.usage.output_tokens;
        usage.cache_read_tokens += record.usage.cache_read_tokens;
        usage.cache_creation_tokens += record.usage.cache_creation_tokens;
        let model_name = record
            .model_name
            .clone()
            .unwrap_or_else(|| parsed.model_name.clone());
        let record_cost = model_cost_usd(&model_name, record.usage);
        total_cost += record_cost;

        let breakdown = model_breakdowns
            .entry(model_name.clone())
            .or_insert_with(|| LocalModelUsage {
                model_name,
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cost: 0.0,
            });
        breakdown.input_tokens += record.usage.input_tokens;
        breakdown.output_tokens += record.usage.output_tokens;
        breakdown.cache_creation_tokens += record.usage.cache_creation_tokens;
        breakdown.cache_read_tokens += record.usage.cache_read_tokens;
        breakdown.cost += record_cost;
    }

    let total_tokens = usage.input_tokens
        + usage.output_tokens
        + usage.cache_creation_tokens
        + usage.cache_read_tokens;
    if total_tokens <= 0 {
        return None;
    }

    let (date, time) = parsed
        .first_time_ms
        .and_then(unix_millis_to_utc_parts)
        .or_else(|| state_created_parts(dir))?;

    Some(LocalSession {
        session_id: format!("{prefix}:{session_id}"),
        date,
        time,
        model_name: parsed.model_name,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        total_tokens_override: None,
        total_cost,
        model_breakdowns: model_breakdowns.into_values().collect(),
    })
}

fn parse_session(dir: &Path) -> Option<ParsedKimiSession> {
    let agents = fs::read_dir(dir.join("agents")).ok()?;
    let mut agent_entries: Vec<_> = agents.flatten().collect();
    agent_entries.sort_by_key(|entry| entry.file_name());

    let mut usage_records = Vec::new();
    let mut first_time_ms: Option<i64> = None;
    let mut main_model_name: Option<String> = None;
    let mut fallback_model_name: Option<String> = None;

    for agent in agent_entries {
        let agent_name = agent.file_name().to_string_lossy().into_owned();
        let wire_path = agent.path().join("wire.jsonl");
        if !wire_path.is_file() {
            continue;
        }
        let Ok(file) = File::open(&wire_path) else {
            continue;
        };
        let mut agent_model_name: Option<String> = None;

        for (line_index, line) in BufReader::new(file).lines().enumerate() {
            let Ok(line) = line else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match value.get("type").and_then(Value::as_str) {
                Some("usage.record") => {
                    let record_model_name = non_empty_string(&value, "model")
                        .filter(|model_name| model_name != SECONDARY_MODEL_ALIAS);
                    if record_model_name.is_some() {
                        agent_model_name = record_model_name.clone();
                    }
                    let time_ms = value.get("time").and_then(Value::as_i64);
                    if let Some(time_ms) = time_ms {
                        first_time_ms = Some(
                            first_time_ms.map_or(time_ms, |current_time| current_time.min(time_ms)),
                        );
                    }
                    let usage = value
                        .get("usage")
                        .map(token_usage_from_value)
                        .unwrap_or_default();
                    usage_records.push(KimiUsageRecord {
                        agent_name: agent_name.clone(),
                        line_number: line_index + 1,
                        time_ms,
                        model_name: record_model_name.or_else(|| agent_model_name.clone()),
                        usage,
                    });
                }
                Some("llm.request") => {
                    let request_model_alias = non_empty_string(&value, "modelAlias")
                        .filter(|model_name| model_name != SECONDARY_MODEL_ALIAS);
                    if let Some(request_model_name) =
                        request_model_alias.or_else(|| non_empty_string(&value, "model"))
                    {
                        agent_model_name = Some(request_model_name);
                    }
                }
                _ => {}
            }
        }

        if agent_name == "main" {
            main_model_name = agent_model_name.clone();
        }
        if fallback_model_name.is_none() {
            fallback_model_name = agent_model_name;
        }
    }

    Some(ParsedKimiSession {
        model_name: main_model_name
            .or(fallback_model_name)
            .unwrap_or_else(|| UNKNOWN_MODEL.to_string()),
        first_time_ms,
        usage_records,
    })
}

fn non_empty_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn token_usage_from_value(value: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: to_i64(&value["inputOther"]),
        output_tokens: to_i64(&value["output"]),
        cache_creation_tokens: to_i64(&value["inputCacheCreation"]),
        cache_read_tokens: to_i64(&value["inputCacheRead"]),
    }
}

fn state_created_parts(dir: &Path) -> Option<(String, String)> {
    let text = fs::read_to_string(dir.join("state.json")).ok()?;
    let state = serde_json::from_str::<Value>(&text).ok()?;
    let created = state.get("createdAt")?;
    let millis = created
        .as_i64()
        .or_else(|| created.as_str().and_then(|text| text.parse::<i64>().ok()));
    if let Some(millis) = millis {
        return unix_millis_to_utc_parts(millis);
    }
    created.as_str().and_then(iso8601_to_local_parts)
}

/// Message-level rows: one per `usage.record` turn, attributed to the turn's
/// own timestamp instead of the session's first/last activity. Records
/// without a `time` are skipped (their tokens remain in session-level views).
pub fn load_messages(watermark_ms: Option<i64>) -> Result<Vec<Value>, SourceError> {
    let mut messages = Vec::new();
    for root in kimi_roots() {
        for dir in session_dirs(&root.path) {
            if let Some(watermark) = watermark_ms {
                if !session_modified_after(&dir, watermark) {
                    continue;
                }
            }
            append_session_messages(root.prefix, &dir, &mut messages);
        }
    }
    Ok(messages)
}

fn append_session_messages(prefix: &str, dir: &Path, messages: &mut Vec<Value>) {
    let Some(session_id) = dir.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let Some(parsed) = parse_session(dir) else {
        return;
    };
    let session_id = format!("{prefix}:{session_id}");
    for record in parsed.usage_records {
        let Some(time_ms) = record.time_ms else {
            continue;
        };
        let Some((date, time)) = unix_millis_to_utc_parts(time_ms) else {
            continue;
        };
        let total_tokens = record.usage.input_tokens
            + record.usage.output_tokens
            + record.usage.cache_creation_tokens
            + record.usage.cache_read_tokens;
        if total_tokens <= 0 {
            continue;
        }
        let model_name = record
            .model_name
            .unwrap_or_else(|| parsed.model_name.clone());
        messages.push(json!({
            "messageId": format!(
                "{prefix}:{}:{}:{}",
                dir.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
                record.agent_name,
                record.line_number
            ),
            "sessionId": session_id,
            "date": date,
            "time": time,
            "modelName": model_name,
            "inputTokens": record.usage.input_tokens,
            "outputTokens": record.usage.output_tokens,
            "cacheCreationTokens": record.usage.cache_creation_tokens,
            "cacheReadTokens": record.usage.cache_read_tokens,
            "totalTokens": total_tokens,
            "cost": model_cost_usd(&model_name, record.usage),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn merges_sub_agent_usage_and_skips_empty_sessions() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-kimi-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let session_dir = root
            .join("sessions")
            .join("wd_demo_0123456789ab")
            .join("session_demo");
        seed_session(&session_dir);

        let empty_dir = root
            .join("sessions")
            .join("wd_demo_0123456789ab")
            .join("session_empty");
        fs::create_dir_all(empty_dir.join("agents").join("main")).unwrap();
        fs::write(
            empty_dir.join("agents").join("main").join("wire.jsonl"),
            "{\"type\":\"metadata\"}\n",
        )
        .unwrap();

        let previous_code = std::env::var_os(KIMI_CODE_HOME_ENV);
        let previous_work = std::env::var_os(KIMI_WORK_HOME_ENV);
        std::env::set_var(KIMI_CODE_HOME_ENV, &root);
        std::env::set_var(KIMI_WORK_HOME_ENV, root.join("missing"));

        let sessions = load_sessions(None).unwrap();

        restore_env(KIMI_CODE_HOME_ENV, previous_code);
        restore_env(KIMI_WORK_HOME_ENV, previous_work);
        let _ = fs::remove_dir_all(&root);

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.session_id, "kimi-code:session_demo");
        assert_eq!(session.input_tokens, 110);
        assert_eq!(session.output_tokens, 25);
        assert_eq!(session.cache_read_tokens, 30);
        assert_eq!(session.cache_creation_tokens, 5);
        assert_eq!(session.model_name, "composer-2.5");
        assert_eq!(session.total_tokens(), 170);
        assert_eq!(session.model_breakdowns.len(), 2);

        let main_usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 20,
            cache_creation_tokens: 5,
            cache_read_tokens: 30,
        };
        let subagent_usage = TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };
        let expected_cost = model_cost_usd("composer-2.5", main_usage)
            + model_cost_usd("composer-2.5-fast", subagent_usage);
        assert_float_eq(session.total_cost, expected_cost);

        let subagent_breakdown = session
            .model_breakdowns
            .iter()
            .find(|breakdown| breakdown.model_name == "composer-2.5-fast")
            .unwrap();
        assert_eq!(subagent_breakdown.input_tokens, 10);
        assert_eq!(subagent_breakdown.output_tokens, 5);
        assert_float_eq(
            subagent_breakdown.cost,
            model_cost_usd("composer-2.5-fast", subagent_usage),
        );
    }

    #[test]
    fn load_messages_emits_one_row_per_usage_record() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-kimi-messages-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let session_dir = root
            .join("sessions")
            .join("wd_demo_0123456789ab")
            .join("session_demo");
        seed_session(&session_dir);

        let previous_code = std::env::var_os(KIMI_CODE_HOME_ENV);
        let previous_work = std::env::var_os(KIMI_WORK_HOME_ENV);
        std::env::set_var(KIMI_CODE_HOME_ENV, &root);
        std::env::set_var(KIMI_WORK_HOME_ENV, root.join("missing"));

        let mut messages = load_messages(None).unwrap();

        restore_env(KIMI_CODE_HOME_ENV, previous_code);
        restore_env(KIMI_WORK_HOME_ENV, previous_work);
        let _ = fs::remove_dir_all(&root);

        messages.sort_by_key(|row| {
            row.get("messageId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        });
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["messageId"], "kimi-code:session_demo:agent-0:2");
        assert_eq!(messages[0]["sessionId"], "kimi-code:session_demo");
        assert_eq!(messages[0]["modelName"], "composer-2.5-fast");
        assert_eq!(messages[0]["totalTokens"], 15);
        assert_eq!(messages[1]["messageId"], "kimi-code:session_demo:main:2");
        assert_eq!(messages[1]["modelName"], "composer-2.5");
        assert_eq!(messages[1]["totalTokens"], 155);
        assert_eq!(messages[1]["inputTokens"], 100);
        assert_eq!(messages[1]["cacheCreationTokens"], 5);
        assert_float_eq(
            messages[0]["cost"].as_f64().unwrap(),
            model_cost_usd(
                "composer-2.5-fast",
                TokenUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                },
            ),
        );
        for message in &messages {
            assert_eq!(message["date"].as_str().unwrap().len(), 10);
            assert_eq!(message["time"].as_str().unwrap().len(), 5);
        }
    }

    #[test]
    fn watermark_skips_unchanged_sessions() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-kimi-watermark-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let session_dir = root
            .join("sessions")
            .join("wd_demo_0123456789ab")
            .join("session_demo");
        seed_session(&session_dir);
        let mtime_ms = [
            session_dir.join("agents").join("main").join("wire.jsonl"),
            session_dir
                .join("agents")
                .join("agent-0")
                .join("wire.jsonl"),
        ]
        .iter()
        .map(file_mtime_ms)
        .max()
        .unwrap();

        let previous_code = std::env::var_os(KIMI_CODE_HOME_ENV);
        let previous_work = std::env::var_os(KIMI_WORK_HOME_ENV);
        std::env::set_var(KIMI_CODE_HOME_ENV, &root);
        std::env::set_var(KIMI_WORK_HOME_ENV, root.join("missing"));

        // First run (no watermark): full scan.
        let full = load_sessions(None).unwrap();
        // Unchanged session (wire mtimes at the watermark): skipped.
        let skipped = load_sessions(Some(mtime_ms)).unwrap();
        // Session newer than the watermark: re-read.
        let reread = load_sessions(Some(mtime_ms - 1)).unwrap();

        restore_env(KIMI_CODE_HOME_ENV, previous_code);
        restore_env(KIMI_WORK_HOME_ENV, previous_work);
        let _ = fs::remove_dir_all(&root);

        assert_eq!(full.len(), 1);
        assert!(skipped.is_empty());
        assert_eq!(reread.len(), 1);
    }

    fn file_mtime_ms(path: &PathBuf) -> i64 {
        let modified = fs::metadata(path).unwrap().modified().unwrap();
        i64::try_from(modified.duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap()
    }

    fn seed_session(dir: &Path) {
        fs::create_dir_all(dir.join("agents").join("main")).unwrap();
        fs::create_dir_all(dir.join("agents").join("agent-0")).unwrap();
        fs::write(
            dir.join("state.json"),
            serde_json::to_string(&json!({
                "id": "session_demo",
                "cwd": "/tmp/demo",
                "createdAt": "1700000000000",
                "title": "demo",
            }))
            .unwrap(),
        )
        .unwrap();
        let main_lines = [
            json!({"type":"llm.request","model":"composer-2.5","modelAlias":"composer-2.5","time":1700000000000i64}),
            json!({"type":"usage.record","model":"composer-2.5","usage":{"inputOther":100,"output":20,"inputCacheRead":30,"inputCacheCreation":5},"usageScope":"turn","time":1700000001000i64}),
        ];
        fs::write(
            dir.join("agents").join("main").join("wire.jsonl"),
            main_lines
                .iter()
                .map(|value| serde_json::to_string(value).unwrap())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        fs::write(
            dir.join("agents").join("agent-0").join("wire.jsonl"),
            [
                json!({"type":"llm.request","model":"composer-2.5-fast","modelAlias":"__secondary__","time":1700000001500i64}),
                json!({"type":"usage.record","model":"__secondary__","usage":{"inputOther":10,"output":5,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"turn","time":1700000002000i64}),
            ]
            .iter()
            .map(|value| serde_json::to_string(value).unwrap())
            .collect::<Vec<_>>()
            .join("\n"),
        )
        .unwrap();
    }

    fn assert_float_eq(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
