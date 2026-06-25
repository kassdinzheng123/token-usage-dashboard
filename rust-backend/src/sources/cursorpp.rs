use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, LocalSession, SourceError};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const CURSORPP_LOG_ROOT_ENV: &str = "CURSORPP_LOG_ROOT";
const UNKNOWN_MODEL: &str = "unknown";

#[derive(Debug, Clone, Default)]
struct PendingRun {
    run_index: usize,
    timestamp: Option<DateTime<Local>>,
    request_id: Option<String>,
    model_name: Option<String>,
    snapshot: Option<UsageSnapshot>,
}

#[derive(Debug, Clone, Default)]
struct UsageSnapshot {
    timestamp: Option<DateTime<Local>>,
    conversation_id: Option<String>,
    totals: UsageTotals,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct UsageTotals {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
}

impl UsageTotals {
    fn total_tokens(self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let Some(root) = discover_log_root() else {
        return Ok(Vec::new());
    };
    if !root.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_cursorpp_logs(&root, &mut files)?;
    files.sort();

    let mut sessions = Vec::new();
    for path in files {
        sessions.extend(read_log_file(&path)?);
    }
    let mut sessions = dedupe_sessions(sessions);
    sessions
        .sort_by_key(|session| format!("{}T{}:{}", session.date, session.time, session.session_id));
    Ok(sessions)
}

fn discover_log_root() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(CURSORPP_LOG_ROOT_ENV) {
        return Some(PathBuf::from(raw));
    }
    home_dir().map(|home| {
        home.join("Library")
            .join("Application Support")
            .join("Cursor")
            .join("logs")
    })
}

fn collect_cursorpp_logs(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), SourceError> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_cursorpp_logs(&path, files)?;
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name.starts_with("Cursor++") && file_name.ends_with(".log") {
            files.push(path);
        }
    }
    Ok(())
}

fn read_log_file(path: &Path) -> Result<Vec<LocalSession>, SourceError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let file_id = session_file_id(path);

    let mut sessions = Vec::new();
    let mut pending: Option<PendingRun> = None;
    let mut run_index = 0usize;
    let mut current_request_id: Option<String> = None;

    for line in reader.lines() {
        let line = line?;
        if let Some(request_id) = parse_run_request_id(&line) {
            if let Some(session) = finalize_pending(&file_id, pending.take()) {
                sessions.push(session);
            }
            current_request_id = Some(request_id);
            continue;
        }
        if current_request_id.is_none() {
            current_request_id = parse_embedded_request_id(&line);
        }

        if let Some((timestamp, model_name)) = parse_agent_start(&line) {
            if let Some(session) = finalize_pending(&file_id, pending.take()) {
                sessions.push(session);
            }
            pending = Some(PendingRun {
                run_index,
                timestamp: Some(timestamp),
                request_id: current_request_id.clone(),
                model_name: Some(model_name),
                snapshot: None,
            });
            run_index += 1;
            continue;
        }

        if let Some(model_name) = parse_oai_response_model(&line) {
            let timestamp = parse_log_timestamp(&line);
            let run = pending.get_or_insert_with(|| {
                let index = run_index;
                run_index += 1;
                PendingRun {
                    run_index: index,
                    timestamp,
                    request_id: current_request_id.clone(),
                    model_name: None,
                    snapshot: None,
                }
            });
            if run.timestamp.is_none() {
                run.timestamp = timestamp;
            }
            if run.request_id.is_none() {
                run.request_id = current_request_id.clone();
            }
            if run.model_name.is_none() {
                run.model_name = Some(model_name);
            }
            continue;
        }

        if let Some(snapshot) = parse_usage_snapshot(&line) {
            let timestamp = snapshot.timestamp;
            let run = pending.get_or_insert_with(|| {
                let index = run_index;
                run_index += 1;
                PendingRun {
                    run_index: index,
                    timestamp,
                    request_id: current_request_id.clone(),
                    model_name: None,
                    snapshot: None,
                }
            });
            if run.timestamp.is_none() {
                run.timestamp = timestamp;
            }
            if run.request_id.is_none() {
                run.request_id = current_request_id.clone();
            }
            run.snapshot = Some(snapshot);
        }

        if is_run_end(&line) {
            if let Some(session) = finalize_pending(&file_id, pending.take()) {
                sessions.push(session);
            }
            current_request_id = None;
        }
    }

    if let Some(session) = finalize_pending(&file_id, pending) {
        sessions.push(session);
    }

    Ok(sessions)
}

fn finalize_pending(file_id: &str, pending: Option<PendingRun>) -> Option<LocalSession> {
    let pending = pending?;
    let snapshot = pending.snapshot.as_ref()?;
    let totals = snapshot.totals;
    if totals.total_tokens() <= 0 {
        return None;
    }

    let timestamp = pending.timestamp.or(snapshot.timestamp)?;
    let model_name = pending
        .model_name
        .clone()
        .unwrap_or_else(|| UNKNOWN_MODEL.to_string());
    let usage = TokenUsage {
        input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        cache_creation_tokens: totals.cache_creation_tokens,
        cache_read_tokens: totals.cache_read_tokens,
    };

    Some(LocalSession {
        session_id: make_session_id(file_id, &pending, snapshot),
        date: timestamp.format("%Y-%m-%d").to_string(),
        time: timestamp.format("%H:%M").to_string(),
        model_name: model_name.clone(),
        input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        cache_creation_tokens: totals.cache_creation_tokens,
        cache_read_tokens: totals.cache_read_tokens,
        total_tokens_override: None,
        total_cost: model_cost_usd(&model_name, usage),
    })
}

fn dedupe_sessions(sessions: Vec<LocalSession>) -> Vec<LocalSession> {
    let mut by_id = BTreeMap::new();
    for session in sessions {
        match by_id.get_mut(&session.session_id) {
            Some(existing) if should_replace_session(&session, existing) => {
                *existing = session;
            }
            Some(_) => {}
            None => {
                by_id.insert(session.session_id.clone(), session);
            }
        }
    }
    by_id.into_values().collect()
}

fn should_replace_session(candidate: &LocalSession, existing: &LocalSession) -> bool {
    if existing.model_name == UNKNOWN_MODEL && candidate.model_name != UNKNOWN_MODEL {
        return true;
    }
    candidate.total_tokens() > existing.total_tokens()
}

fn make_session_id(file_id: &str, pending: &PendingRun, snapshot: &UsageSnapshot) -> String {
    if let Some(request_id) = pending.request_id.as_deref().filter(|id| !id.is_empty()) {
        return format!("cursorpp:{request_id}");
    }

    if let Some(conversation_id) = snapshot
        .conversation_id
        .as_deref()
        .filter(|id| !id.is_empty())
    {
        let timestamp = snapshot
            .timestamp
            .or(pending.timestamp)
            .map(|timestamp| timestamp.format("%Y%m%d%H%M%S").to_string())
            .unwrap_or_else(|| "unknown-time".to_string());
        return format!("cursorpp:{conversation_id}:{timestamp}");
    }

    format!("{file_id}:{}", pending.run_index)
}

fn session_file_id(path: &Path) -> String {
    let mut id = String::new();
    let mut previous_was_separator = false;

    for character in path.to_string_lossy().chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+') {
            id.push(character);
            previous_was_separator = false;
        } else if !previous_was_separator {
            id.push('_');
            previous_was_separator = true;
        }
    }

    let trimmed = id.trim_matches('_');
    if trimmed.is_empty() {
        "cursorpp".to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_run_request_id(line: &str) -> Option<String> {
    if !line.contains("AgentService/RunSSE started") {
        return None;
    }
    let value = parse_json_value_after(line)?;
    value
        .get("requestId")
        .and_then(Value::as_str)
        .filter(|request_id| !request_id.is_empty())
        .map(ToString::to_string)
}

fn parse_embedded_request_id(line: &str) -> Option<String> {
    let marker = "\"requestId\":";
    if !line.contains(marker) {
        return None;
    }
    let value = parse_json_value_after(line)?;
    value
        .get("requestId")
        .and_then(Value::as_str)
        .filter(|request_id| !request_id.is_empty())
        .map(ToString::to_string)
}

fn is_run_end(line: &str) -> bool {
    line.contains("AgentService/RunSSE \u{2192}")
}

fn parse_agent_start(line: &str) -> Option<(DateTime<Local>, String)> {
    let timestamp = parse_log_timestamp(line)?;
    let marker = "[AGENT] \u{2192} [";
    let marker_index = line.find(marker)?;
    let after_marker = &line[marker_index + marker.len()..];
    let end = after_marker.find(']')?;
    let provider_model = &after_marker[..end];
    let model_name = provider_model
        .rsplit_once('/')
        .map(|(_, model)| model)
        .unwrap_or(provider_model)
        .trim();
    if model_name.is_empty() {
        return None;
    }
    Some((timestamp, model_name.to_string()))
}

fn parse_oai_response_model(line: &str) -> Option<String> {
    if !line.contains("[OAI_RESP] prompt cache") {
        return None;
    }
    let value = parse_json_value_after(line)?;
    value
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model_name| !model_name.is_empty())
        .map(ToString::to_string)
}

fn parse_usage_snapshot(line: &str) -> Option<UsageSnapshot> {
    if !line.contains("\"usageTotals\":") {
        return None;
    }
    let value = parse_json_value_after(line)?;
    let usage = value.get("usageTotals")?;
    let raw_input_tokens = integer_field(usage, "inputTokens");
    let cache_creation_tokens = integer_field(usage, "cacheWriteTokens");
    let cache_read_tokens = integer_field(usage, "cacheReadTokens");
    Some(UsageSnapshot {
        timestamp: parse_log_timestamp(line),
        conversation_id: value
            .get("conversationId")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        totals: UsageTotals {
            input_tokens: normalize_input_tokens(
                raw_input_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            ),
            output_tokens: integer_field(usage, "outputTokens"),
            cache_creation_tokens,
            cache_read_tokens,
        },
    })
}

fn normalize_input_tokens(
    raw_input_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
) -> i64 {
    raw_input_tokens
        .saturating_sub(cache_creation_tokens)
        .saturating_sub(cache_read_tokens)
        .max(0)
}

fn parse_json_value_after(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    for (offset, character) in text[start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&text[start..start + offset + 1]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_log_timestamp(line: &str) -> Option<DateTime<Local>> {
    let raw = line.get(..19)?;
    let naive = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S").ok()?;
    Local.from_local_datetime(&naive).single()
}

fn integer_field(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cursorpp_run_without_counting_intermediate_totals() {
        let lines = [
            r#"2026-06-07 13:36:40.007 [info] [SVC] AgentService/RunSSE started {"requestId":"7f0a8cf0-edc2-4bdd-949e-d01af6b5769e"}"#,
            r#"2026-06-07 13:36:40.208 [info] [AGENT] → [openai-responses/gpt-5.5] "prompt omitted" (236 msgs)"#,
            r#"2026-06-07 13:37:12.934 [info] [SESSION] round checkpoint update {"conversationId":"c1","round":0,"usageTotals":{"inputTokens":157266,"outputTokens":741,"cacheReadTokens":0,"cacheWriteTokens":0}}"#,
            r#"2026-06-07 13:38:24.598 [info] [SESSION] checkpoint assistant content {"conversationId":"c1","usageTotals":{"inputTokens":318473,"outputTokens":3881,"cacheReadTokens":157184,"cacheWriteTokens":0}}"#,
        ];

        let mut pending = None;
        let mut run_index = 0usize;
        let mut current_request_id = None;
        for line in lines {
            if let Some(request_id) = parse_run_request_id(line) {
                current_request_id = Some(request_id);
                continue;
            }
            if let Some((timestamp, model_name)) = parse_agent_start(line) {
                pending = Some(PendingRun {
                    run_index,
                    timestamp: Some(timestamp),
                    request_id: current_request_id.clone(),
                    model_name: Some(model_name),
                    snapshot: None,
                });
                run_index += 1;
                continue;
            }
            if let Some(snapshot) = parse_usage_snapshot(line) {
                pending.as_mut().unwrap().snapshot = Some(snapshot);
            }
        }

        let session = finalize_pending("Cursor++.log", pending).unwrap();
        assert_eq!(
            session.session_id,
            "cursorpp:7f0a8cf0-edc2-4bdd-949e-d01af6b5769e"
        );
        assert_eq!(session.model_name, "gpt-5.5");
        assert_eq!(session.input_tokens, 161_289);
        assert_eq!(session.output_tokens, 3_881);
        assert_eq!(session.cache_read_tokens, 157_184);
        assert_eq!(session.total_tokens(), 322_354);
    }

    #[test]
    fn parses_usage_totals_object() {
        let line = r#"2026-06-07 13:20:19.851 [info] [SESSION] round checkpoint update {"conversationId":"c1","usageTotals":{"inputTokens":71824,"outputTokens":807,"cacheReadTokens":0,"cacheWriteTokens":12}}"#;
        let snapshot = parse_usage_snapshot(line).unwrap();
        assert_eq!(snapshot.conversation_id.as_deref(), Some("c1"));
        assert_eq!(
            snapshot.totals,
            UsageTotals {
                input_tokens: 71_812,
                output_tokens: 807,
                cache_creation_tokens: 12,
                cache_read_tokens: 0,
            }
        );
    }

    #[test]
    fn clamps_normalized_input_tokens_at_zero() {
        assert_eq!(normalize_input_tokens(5, 4, 6), 0);
    }

    #[test]
    fn parses_model_from_agent_line() {
        let line = r#"2026-06-07 13:19:59.662 [info] [AGENT] → [openai-responses/gpt-5.5] "prompt omitted" (120 msgs)"#;
        let (_, model_name) = parse_agent_start(line).unwrap();
        assert_eq!(model_name, "gpt-5.5");
    }

    #[test]
    fn dedupes_mirrored_window_logs_by_request_id() {
        let session = LocalSession {
            session_id: "cursorpp:r1".to_string(),
            date: "2026-06-07".to_string(),
            time: "13:20".to_string(),
            model_name: "gpt-5.5".to_string(),
            input_tokens: 10,
            output_tokens: 2,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            total_tokens_override: None,
            total_cost: 0.0,
        };
        let mut duplicate = session.clone();
        duplicate.input_tokens = 20;

        let sessions = dedupe_sessions(vec![session, duplicate]);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "cursorpp:r1");
        assert_eq!(sessions[0].input_tokens, 20);
    }

    #[test]
    fn recovers_request_and_model_from_rotated_log_fragment() {
        let temp_root = std::env::temp_dir().join(format!(
            "token-usage-cursorpp-rotated-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_root);
        fs::create_dir_all(&temp_root).unwrap();
        let log_path = temp_root.join("Cursor++.fragment.log");
        fs::write(
            &log_path,
            [
                r#"2026-06-07 13:19:58.662 [info] [SESSION] appendMessage {"requestId":"r1","keys":["clientHeartbeat"],"protoBytes":2}"#,
                r#"2026-06-07 13:20:18.662 [info] [OAI_RESP] prompt cache {"model":"gpt-5.5","cached":10,"input":20}"#,
                r#"2026-06-07 13:20:19.851 [info] [SESSION] checkpoint assistant content {"conversationId":"c1","usageTotals":{"inputTokens":20,"outputTokens":2,"cacheReadTokens":10,"cacheWriteTokens":0}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let sessions = read_log_file(&log_path).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "cursorpp:r1");
        assert_eq!(sessions[0].model_name, "gpt-5.5");
        assert_eq!(sessions[0].total_tokens(), 22);

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn aggregates_realistic_lines_into_two_sessions() {
        let temp_root =
            std::env::temp_dir().join(format!("token-usage-cursorpp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_root);
        fs::create_dir_all(&temp_root).unwrap();
        let log_path = temp_root.join("Cursor++.test.log");
        fs::write(
            &log_path,
            [
                r#"2026-06-07 13:19:58.662 [info] [SVC] AgentService/RunSSE started {"requestId":"r1"}"#,
                r#"2026-06-07 13:19:59.662 [info] [AGENT] → [openai-responses/gpt-5.5] "first" (120 msgs)"#,
                r#"2026-06-07 13:20:19.851 [info] [SESSION] round checkpoint update {"conversationId":"c1","usageTotals":{"inputTokens":10,"outputTokens":2,"cacheReadTokens":0,"cacheWriteTokens":0}}"#,
                r#"2026-06-07 13:20:41.563 [info] [SVC] AgentService/RunSSE started {"requestId":"r2"}"#,
                r#"2026-06-07 13:20:42.563 [info] [AGENT] → [anthropic/deepseek-v4-flash] "" (120 msgs)"#,
                r#"2026-06-07 13:21:49.337 [info] [SESSION] round checkpoint update {"conversationId":"c2","usageTotals":{"inputTokens":30,"outputTokens":4,"cacheReadTokens":5,"cacheWriteTokens":6}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let sessions = read_log_file(&log_path).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "cursorpp:r1");
        assert_eq!(sessions[0].model_name, "gpt-5.5");
        assert_eq!(sessions[0].total_tokens(), 12);
        assert_eq!(sessions[1].session_id, "cursorpp:r2");
        assert_eq!(sessions[1].model_name, "deepseek-v4-flash");
        assert_eq!(sessions[1].input_tokens, 19);
        assert_eq!(sessions[1].output_tokens, 4);
        assert_eq!(sessions[1].cache_creation_tokens, 6);
        assert_eq!(sessions[1].cache_read_tokens, 5);
        assert_eq!(sessions[1].total_tokens(), 34);

        let _ = fs::remove_dir_all(&temp_root);
    }
}
