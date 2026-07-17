use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{
    home_dir, iso8601_to_local_parts, to_i64, unix_millis_to_utc_parts, LocalSession,
    SourceError,
};
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const KIMI_CODE_HOME_ENV: &str = "KIMI_CODE_HOME";
const KIMI_WORK_HOME_ENV: &str = "KIMI_WORK_HOME";
const KIMI_WORK_RELATIVE_HOME: &str =
    "Library/Application Support/kimi-desktop/daimon-share/daimon/runtime/kimi-code/home";
const UNKNOWN_MODEL: &str = "unknown";

struct KimiRoot {
    prefix: &'static str,
    path: PathBuf,
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

    let mut usage = TokenUsage::default();
    let mut model_name: Option<String> = None;
    let mut model_alias: Option<String> = None;
    let mut first_time: Option<i64> = None;

    let agents_dir = dir.join("agents");
    let agents = fs::read_dir(&agents_dir).ok()?;
    for agent in agents.flatten() {
        let wire_path = agent.path().join("wire.jsonl");
        if !wire_path.is_file() {
            continue;
        }
        let Ok(file) = File::open(&wire_path) else {
            continue;
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match value.get("type").and_then(Value::as_str) {
                Some("usage.record") => {
                    if let Some(record) = value.get("usage") {
                        usage.input_tokens += to_i64(&record["inputOther"]);
                        usage.output_tokens += to_i64(&record["output"]);
                        usage.cache_read_tokens += to_i64(&record["inputCacheRead"]);
                        usage.cache_creation_tokens += to_i64(&record["inputCacheCreation"]);
                    }
                    if let Some(time) = value.get("time").and_then(Value::as_i64) {
                        first_time = Some(
                            first_time.map_or(time, |current: i64| current.min(time)),
                        );
                    }
                    if let Some(model) = value
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        model_name = Some(model.to_string());
                    }
                }
                Some("llm.request") => {
                    if let Some(alias) = value
                        .get("modelAlias")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        model_alias = Some(alias.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    let total_tokens = usage.input_tokens
        + usage.output_tokens
        + usage.cache_creation_tokens
        + usage.cache_read_tokens;
    if total_tokens <= 0 {
        return None;
    }

    let (date, time) = first_time
        .and_then(unix_millis_to_utc_parts)
        .or_else(|| state_created_parts(dir))?;
    let model_name = model_name
        .or(model_alias)
        .unwrap_or_else(|| UNKNOWN_MODEL.to_string());

    Some(LocalSession {
        session_id: format!("{prefix}:{session_id}"),
        date,
        time,
        model_name: model_name.clone(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        total_tokens_override: None,
        total_cost: model_cost_usd(&model_name, usage),
    })
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
    let Ok(agents) = fs::read_dir(dir.join("agents")) else {
        return;
    };

    // Resolve the session-level model exactly like `load_session` (last record
    // model wins, then the last request alias), then price each turn with its
    // own model when present.
    let mut pending: Vec<(String, String, String, Option<String>, TokenUsage)> = Vec::new();
    let mut model_name: Option<String> = None;
    let mut model_alias: Option<String> = None;

    for agent in agents.flatten() {
        let agent_name = agent.file_name().to_string_lossy().into_owned();
        let wire_path = agent.path().join("wire.jsonl");
        if !wire_path.is_file() {
            continue;
        }
        let Ok(file) = File::open(&wire_path) else {
            continue;
        };
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let Ok(line) = line else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match value.get("type").and_then(Value::as_str) {
                Some("usage.record") => {
                    let Some(time) = value.get("time").and_then(Value::as_i64) else {
                        continue;
                    };
                    let Some((date, time_part)) = unix_millis_to_utc_parts(time) else {
                        continue;
                    };
                    let Some(record) = value.get("usage") else {
                        continue;
                    };
                    let usage = TokenUsage {
                        input_tokens: to_i64(&record["inputOther"]),
                        output_tokens: to_i64(&record["output"]),
                        cache_creation_tokens: to_i64(&record["inputCacheCreation"]),
                        cache_read_tokens: to_i64(&record["inputCacheRead"]),
                    };
                    if usage.input_tokens
                        + usage.output_tokens
                        + usage.cache_creation_tokens
                        + usage.cache_read_tokens
                        <= 0
                    {
                        continue;
                    }
                    if let Some(model) = value
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        model_name = Some(model.to_string());
                    }
                    let record_model = value
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(ToOwned::to_owned);
                    pending.push((
                        format!("{prefix}:{session_id}:{agent_name}:{}", index + 1),
                        date,
                        time_part,
                        record_model,
                        usage,
                    ));
                }
                Some("llm.request") => {
                    if let Some(alias) = value
                        .get("modelAlias")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        model_alias = Some(alias.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    let fallback_model = model_name
        .or(model_alias)
        .unwrap_or_else(|| UNKNOWN_MODEL.to_string());
    let session_id = format!("{prefix}:{session_id}");
    for (message_id, date, time, record_model, usage) in pending {
        let model_name = record_model.unwrap_or_else(|| fallback_model.clone());
        let total_tokens = usage.input_tokens
            + usage.output_tokens
            + usage.cache_creation_tokens
            + usage.cache_read_tokens;
        messages.push(json!({
            "messageId": message_id,
            "sessionId": session_id,
            "date": date,
            "time": time,
            "inputTokens": usage.input_tokens,
            "outputTokens": usage.output_tokens,
            "cacheCreationTokens": usage.cache_creation_tokens,
            "cacheReadTokens": usage.cache_read_tokens,
            "totalTokens": total_tokens,
            "cost": model_cost_usd(&model_name, usage),
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
        assert_eq!(session.model_name, "kimi-code/k3");
        assert_eq!(session.total_tokens(), 170);
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
        assert_eq!(
            messages[0]["messageId"],
            "kimi-code:session_demo:agent-0:1"
        );
        assert_eq!(messages[0]["sessionId"], "kimi-code:session_demo");
        assert_eq!(messages[0]["totalTokens"], 15);
        assert_eq!(messages[1]["messageId"], "kimi-code:session_demo:main:2");
        assert_eq!(messages[1]["totalTokens"], 155);
        assert_eq!(messages[1]["inputTokens"], 100);
        assert_eq!(messages[1]["cacheCreationTokens"], 5);
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
            session_dir.join("agents").join("agent-0").join("wire.jsonl"),
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
        i64::try_from(
            modified
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
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
            json!({"type":"llm.request","model":"k3","modelAlias":"kimi-code/k3","time":1700000000000i64}),
            json!({"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther":100,"output":20,"inputCacheRead":30,"inputCacheCreation":5},"usageScope":"turn","time":1700000001000i64}),
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
            "{\"type\":\"usage.record\",\"usage\":{\"inputOther\":10,\"output\":5,\"inputCacheRead\":0,\"inputCacheCreation\":0},\"usageScope\":\"turn\",\"time\":1700000002000}",
        )
        .unwrap();
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
