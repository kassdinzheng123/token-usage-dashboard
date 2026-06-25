use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, LocalSession, SourceError};
use chrono::{DateTime, Local};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const GROK_HOME_ENV: &str = "GROK_HOME";
const GROK_LOG_ROOT_ENV: &str = "GROK_LOG_ROOT";
const UNKNOWN_MODEL: &str = "unknown";

#[derive(Debug, Clone, Default)]
struct SessionMetadata {
    model_name: String,
}

#[derive(Debug, Clone)]
struct InferenceEvent {
    session_id: String,
    loop_index: i64,
    timestamp: DateTime<Local>,
    prompt_tokens: i64,
    cached_prompt_tokens: i64,
    completion_tokens: i64,
    reasoning_tokens: i64,
}

pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let metadata = load_session_metadata()?;
    let events = load_inference_events()?;
    let mut sessions = Vec::with_capacity(events.len());

    for event in events {
        if let Some(session) = inference_to_session(&event, &metadata) {
            sessions.push(session);
        }
    }

    sessions.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then_with(|| left.time.cmp(&right.time))
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    Ok(sessions)
}

fn inference_to_session(
    event: &InferenceEvent,
    metadata: &BTreeMap<String, SessionMetadata>,
) -> Option<LocalSession> {
    let cache_read_tokens = event.cached_prompt_tokens.max(0);
    let input_tokens = event
        .prompt_tokens
        .saturating_sub(cache_read_tokens)
        .max(0);
    let output_tokens = event
        .completion_tokens
        .saturating_add(event.reasoning_tokens)
        .max(0);
    let total_tokens = input_tokens + output_tokens + cache_read_tokens;
    if total_tokens <= 0 {
        return None;
    }

    let session_meta = metadata.get(&event.session_id);
    let model_name = session_meta
        .map(|meta| meta.model_name.clone())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| UNKNOWN_MODEL.to_string());
    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_creation_tokens: 0,
        cache_read_tokens,
    };

    Some(LocalSession {
        session_id: format!("grok:{}:{}", event.session_id, event.loop_index),
        date: event.timestamp.format("%Y-%m-%d").to_string(),
        time: event.timestamp.format("%H:%M").to_string(),
        model_name: model_name.clone(),
        input_tokens,
        output_tokens,
        cache_creation_tokens: 0,
        cache_read_tokens,
        total_tokens_override: None,
        total_cost: model_cost_usd(&model_name, usage),
    })
}

fn load_inference_events() -> Result<Vec<InferenceEvent>, SourceError> {
    let Some(log_root) = discover_log_root() else {
        return Ok(Vec::new());
    };
    if !log_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_log_files(&log_root, &mut files)?;
    files.sort();

    let mut events = Vec::new();
    for path in files {
        events.extend(read_log_file(&path)?);
    }
    Ok(events)
}

fn load_session_metadata() -> Result<BTreeMap<String, SessionMetadata>, SourceError> {
    let Some(sessions_root) = discover_sessions_root() else {
        return Ok(BTreeMap::new());
    };
    if !sessions_root.is_dir() {
        return Ok(BTreeMap::new());
    }

    let mut metadata = BTreeMap::new();
    collect_session_metadata(&sessions_root, &mut metadata)?;
    Ok(metadata)
}

fn collect_session_metadata(
    dir: &Path,
    metadata: &mut BTreeMap<String, SessionMetadata>,
) -> Result<(), SourceError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_session_metadata(&path, metadata)?;
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) != Some("summary.json") {
            continue;
        }
        if let Some(session_id) = parse_summary_file(&path) {
            metadata.insert(session_id.0, session_id.1);
        }
    }
    Ok(())
}

fn parse_summary_file(path: &Path) -> Option<(String, SessionMetadata)> {
    let contents = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&contents).ok()?;
    let session_id = value
        .get("info")
        .and_then(|info| info.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)?;
    let model_name = value
        .get("current_model_id")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .unwrap_or(UNKNOWN_MODEL)
        .to_owned();
    Some((
        session_id,
        SessionMetadata { model_name },
    ))
}

fn read_log_file(path: &Path) -> Result<Vec<InferenceEvent>, SourceError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(event) = parse_inference_event(trimmed) {
            events.push(event);
        }
    }

    Ok(events)
}

fn parse_inference_event(line: &str) -> Option<InferenceEvent> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    if value.get("msg").and_then(Value::as_str) != Some("shell.turn.inference_done") {
        return None;
    }

    let session_id = value.get("sid").and_then(Value::as_str)?.to_owned();
    let timestamp = value
        .get("ts")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)?;
    let ctx = value.get("ctx")?;
    let prompt_tokens = integer_field(ctx, "prompt_tokens");
    let cached_prompt_tokens = integer_field(ctx, "cached_prompt_tokens");
    let completion_tokens = integer_field(ctx, "completion_tokens");
    let reasoning_tokens = integer_field(ctx, "reasoning_tokens");
    let loop_index = integer_field(ctx, "loop_index");
    if loop_index <= 0 {
        return None;
    }

    Some(InferenceEvent {
        session_id,
        loop_index,
        timestamp,
        prompt_tokens,
        cached_prompt_tokens,
        completion_tokens,
        reasoning_tokens,
    })
}

fn parse_timestamp(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Local))
}

fn integer_field(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
                .or_else(|| value.as_f64().filter(|number| number.is_finite()).map(|n| n as i64))
        })
        .unwrap_or_default()
}

fn discover_grok_home() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(GROK_HOME_ENV) {
        return Some(PathBuf::from(raw));
    }
    home_dir().map(|home| home.join(".grok"))
}

fn discover_log_root() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(GROK_LOG_ROOT_ENV) {
        return Some(PathBuf::from(raw));
    }
    discover_grok_home().map(|home| home.join("logs"))
}

fn discover_sessions_root() -> Option<PathBuf> {
    discover_grok_home().map(|home| home.join("sessions"))
}

fn collect_log_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), SourceError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_log_files(&path, files)?;
            continue;
        }
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "jsonl")
        {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parse_inference_event_maps_tokens_per_loop() {
        let line = r#"{"ts":"2026-06-25T10:39:32.723Z","src":"shell","sid":"session-1","msg":"shell.turn.inference_done","ctx":{"loop_index":2,"prompt_tokens":15233,"cached_prompt_tokens":14017,"completion_tokens":124,"reasoning_tokens":3}}"#;
        let event = parse_inference_event(line).unwrap();
        assert_eq!(event.session_id, "session-1");
        assert_eq!(event.loop_index, 2);
        assert_eq!(event.prompt_tokens, 15_233);
        assert_eq!(event.cached_prompt_tokens, 14_017);
        assert_eq!(event.completion_tokens, 124);
        assert_eq!(event.reasoning_tokens, 3);
        assert_eq!(event.timestamp.format("%Y-%m-%d").to_string(), "2026-06-25");
    }

    #[test]
    fn inference_to_session_uses_per_loop_session_id() {
        let event = InferenceEvent {
            session_id: "session-1".to_owned(),
            loop_index: 4,
            timestamp: Local.with_ymd_and_hms(2026, 6, 25, 10, 39, 51).unwrap(),
            prompt_tokens: 26_534,
            cached_prompt_tokens: 22_619,
            completion_tokens: 194,
            reasoning_tokens: 0,
        };
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "session-1".to_owned(),
            SessionMetadata {
                model_name: "grok-composer-2.5-fast".to_owned(),
            },
        );

        let session = inference_to_session(&event, &metadata).unwrap();
        assert_eq!(session.session_id, "grok:session-1:4");
        assert_eq!(session.model_name, "grok-composer-2.5-fast");
        assert_eq!(session.input_tokens, 3_915);
        assert_eq!(session.output_tokens, 194);
        assert_eq!(session.cache_read_tokens, 22_619);
        assert_eq!(session.total_tokens(), 26_728);
    }

    #[test]
    fn load_sessions_reads_fixture_log_and_summary() {
        let temp_root =
            std::env::temp_dir().join(format!("token-usage-grok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_root);
        let logs_dir = temp_root.join("logs");
        let session_dir = temp_root
            .join("sessions")
            .join("encoded-cwd")
            .join("session-1");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::create_dir_all(&session_dir).unwrap();

        fs::write(
            logs_dir.join("unified.jsonl"),
            [
                r#"{"ts":"2026-06-25T10:39:32.723Z","sid":"session-1","msg":"shell.turn.inference_done","ctx":{"loop_index":1,"prompt_tokens":100,"cached_prompt_tokens":40,"completion_tokens":10,"reasoning_tokens":1}}"#,
                r#"{"ts":"2026-06-25T10:39:35.038Z","sid":"session-1","msg":"shell.turn.inference_done","ctx":{"loop_index":2,"prompt_tokens":150,"cached_prompt_tokens":90,"completion_tokens":5,"reasoning_tokens":0}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        fs::write(
            session_dir.join("summary.json"),
            r#"{"info":{"id":"session-1","cwd":"/repo"},"current_model_id":"grok-composer-2.5-fast"}"#,
        )
        .unwrap();

        let previous_home = std::env::var_os(GROK_HOME_ENV);
        std::env::set_var(GROK_HOME_ENV, &temp_root);

        let sessions = load_sessions().unwrap();
        if let Some(value) = previous_home {
            std::env::set_var(GROK_HOME_ENV, value);
        } else {
            std::env::remove_var(GROK_HOME_ENV);
        }

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "grok:session-1:1");
        assert_eq!(sessions[1].session_id, "grok:session-1:2");
        assert_eq!(sessions[0].input_tokens, 60);
        assert_eq!(sessions[0].output_tokens, 11);
        assert_eq!(sessions[1].input_tokens, 60);
        assert_eq!(sessions[1].output_tokens, 5);

        let _ = fs::remove_dir_all(&temp_root);
    }
}