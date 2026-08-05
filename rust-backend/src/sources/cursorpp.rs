use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, LocalSession, SourceError};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
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

pub fn load_sessions(watermark_ms: Option<i64>) -> Result<Vec<LocalSession>, SourceError> {
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
        if let Some(watermark) = watermark_ms {
            if !super::file_modified_after(&path, watermark) {
                continue;
            }
        }
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

    // Multi-pending architecture: each concurrent RunSSE gets its own PendingRun
    // keyed by requestId. This prevents concurrent subagent sessions from
    // prematurely finalizing each other's pending state.
    let mut pending_by_request_id: HashMap<String, PendingRun> = HashMap::new();
    let mut conversation_id_to_request_id: HashMap<String, String> = HashMap::new();
    let mut run_index = 0usize;
    let mut current_request_id: Option<String> = None;
    let mut sessions = Vec::new();

    for line in reader.lines() {
        let line = line?;

        // RunSSE started — register a new pending entry for this requestId.
        if let Some(request_id) = parse_run_request_id(&line) {
            current_request_id = Some(request_id.clone());
            pending_by_request_id.entry(request_id.clone()).or_insert_with(|| {
                run_index += 1;
                PendingRun {
                    run_index,
                    timestamp: parse_log_timestamp(&line),
                    request_id: Some(request_id),
                    model_name: None,
                    snapshot: None,
                }
            });
            continue;
        }

        // Fallback: infer requestId from embedded JSON if we don't have one yet.
        if current_request_id.is_none() {
            current_request_id = parse_embedded_request_id(&line);
        }

        // Proactively map any conversationId seen in this line to the current
        // requestId. This ensures that when usageTotals arrive later (possibly
        // after current_request_id has switched to a concurrent session), we
        // can still route them back to the correct pending run.
        if let Some(request_id) = &current_request_id {
            if let Some(conv_id) = parse_line_conversation_id(&line) {
                conversation_id_to_request_id
                    .entry(conv_id)
                    .or_insert(request_id.clone());
            }
        }

        // [AGENT] line provides the model name. Attach it to the pending run
        // for the current requestId (or create one if missing).
        if let Some((timestamp, model_name)) = parse_agent_start(&line) {
            let request_id = resolve_request_id_for_agent_line(
                &line,
                &current_request_id,
                &mut pending_by_request_id,
                &mut run_index,
            );
            let run = pending_by_request_id
                .entry(request_id.clone())
                .or_insert_with(|| {
                    run_index += 1;
                    PendingRun {
                        run_index,
                        timestamp: Some(timestamp),
                        request_id: Some(request_id),
                        model_name: None,
                        snapshot: None,
                    }
                });
            if run.timestamp.is_none() {
                run.timestamp = Some(timestamp);
            }
            if run.model_name.is_none() {
                run.model_name = Some(model_name);
            }
            continue;
        }

        // Prompt-cache lines provide model name + timestamp. Attach to the
        // current requestId's pending run.
        if let Some(model_name) = parse_prompt_cache_model(&line) {
            let timestamp = parse_log_timestamp(&line);
            if let Some(request_id) = &current_request_id {
                let run = pending_by_request_id
                    .entry(request_id.clone())
                    .or_insert_with(|| {
                        run_index += 1;
                        PendingRun {
                            run_index,
                            timestamp,
                            request_id: Some(request_id.clone()),
                            model_name: None,
                            snapshot: None,
                        }
                    });
                if run.timestamp.is_none() {
                    run.timestamp = timestamp;
                }
                if run.model_name.is_none() {
                    run.model_name = Some(model_name);
                }
            }
            continue;
        }

        // usageTotals snapshot — the key event that carries token counts.
        // We need to route it to the correct pending run by conversationId
        // (not all usageTotals lines contain a requestId).
        if let Some(snapshot) = parse_usage_snapshot(&line) {
            let request_id = resolve_request_id_for_snapshot(
                &snapshot,
                &current_request_id,
                &mut conversation_id_to_request_id,
                &mut pending_by_request_id,
                &mut run_index,
                &line,
            );

            let timestamp = snapshot.timestamp;
            let run = pending_by_request_id
                .entry(request_id.clone())
                .or_insert_with(|| {
                    run_index += 1;
                    PendingRun {
                        run_index,
                        timestamp,
                        request_id: Some(request_id.clone()),
                        model_name: None,
                        snapshot: None,
                    }
                });
            if run.timestamp.is_none() {
                run.timestamp = timestamp;
            }
            if run.request_id.is_none() {
                run.request_id = Some(request_id.clone());
            }
            run.snapshot = Some(snapshot);
        }
    }

    // Finalize all remaining pending runs at end of file.
    for (_, pending) in pending_by_request_id {
        if let Some(session) = finalize_pending(&file_id, pending) {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

fn finalize_pending(file_id: &str, pending: PendingRun) -> Option<LocalSession> {
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
        model_breakdowns: Vec::new(),
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

/// Extract conversationId from any log line that contains one in its JSON
/// payload. Used to proactively build the conversationId -> requestId map
/// so that later usageTotals lines can be routed to the correct pending run
/// even when concurrent sessions interleave.
fn parse_line_conversation_id(line: &str) -> Option<String> {
    if !line.contains("\"conversationId\"") {
        return None;
    }
    let value = parse_json_value_after(line)?;
    value
        .get("conversationId")
        .and_then(Value::as_str)
        .filter(|conv_id| !conv_id.is_empty())
        .map(ToString::to_string)
}

/// Resolve which requestId an [AGENT] line belongs to. In most cases the
/// current_request_id is the active session, but when subagents interleave,
/// the AGENT line's embedded requestId (if any) takes precedence.
fn resolve_request_id_for_agent_line(
    line: &str,
    current_request_id: &Option<String>,
    pending_by_request_id: &mut HashMap<String, PendingRun>,
    run_index: &mut usize,
) -> String {
    if let Some(embedded) = parse_embedded_request_id(line) {
        return embedded;
    }
    if let Some(request_id) = current_request_id {
        return request_id.clone();
    }
    // No requestId available — create a synthetic one.
    let synthetic = format!("synthetic-agent-{}", *run_index);
    *run_index += 1;
    pending_by_request_id.insert(
        synthetic.clone(),
        PendingRun {
            run_index: *run_index,
            ..Default::default()
        },
    );
    synthetic
}

/// Resolve which requestId a usageTotals snapshot belongs to. The snapshot
/// carries a conversationId which we map to the requestId that was active
/// when we first saw that conversationId. This handles interleaved concurrent
/// sessions where usageTotals from session A arrive after session B has
/// already started.
fn resolve_request_id_for_snapshot(
    snapshot: &UsageSnapshot,
    current_request_id: &Option<String>,
    conversation_id_to_request_id: &mut HashMap<String, String>,
    pending_by_request_id: &mut HashMap<String, PendingRun>,
    run_index: &mut usize,
    line: &str,
) -> String {
    // 1. If the snapshot line itself contains a requestId, use it directly.
    if let Some(embedded) = parse_embedded_request_id(line) {
        if let Some(conv_id) = &snapshot.conversation_id {
            conversation_id_to_request_id.insert(conv_id.clone(), embedded.clone());
        }
        return embedded;
    }

    // 2. If we've seen this conversationId before, route to the known requestId.
    if let Some(conv_id) = &snapshot.conversation_id {
        if let Some(request_id) = conversation_id_to_request_id.get(conv_id) {
            return request_id.clone();
        }
    }

    // 3. Fall back to the current requestId.
    if let Some(request_id) = current_request_id {
        if let Some(conv_id) = &snapshot.conversation_id {
            conversation_id_to_request_id.insert(conv_id.clone(), request_id.clone());
        }
        return request_id.clone();
    }

    // 4. No requestId and no conversationId mapping — create a synthetic entry.
    let conv_key = snapshot
        .conversation_id
        .as_deref()
        .unwrap_or("unknown-conv");
    let synthetic = format!("cursorpp-synthetic-{}", conv_key);
    if let Some(conv_id) = &snapshot.conversation_id {
        conversation_id_to_request_id.insert(conv_id.clone(), synthetic.clone());
    }
    pending_by_request_id.insert(
        synthetic.clone(),
        PendingRun {
            run_index: *run_index,
            ..Default::default()
        },
    );
    *run_index += 1;
    synthetic
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

fn parse_prompt_cache_model(line: &str) -> Option<String> {
    if !line.contains("[OAI_RESP] prompt cache") && !line.contains("[ANTHROPIC] prompt cache") {
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

        let session = finalize_pending("Cursor++.log", pending.unwrap()).unwrap();
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
            model_breakdowns: Vec::new(),
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

        let r1 = sessions
            .iter()
            .find(|s| s.session_id == "cursorpp:r1")
            .expect("r1 session should exist");
        assert_eq!(r1.model_name, "gpt-5.5");
        assert_eq!(r1.total_tokens(), 12);

        let r2 = sessions
            .iter()
            .find(|s| s.session_id == "cursorpp:r2")
            .expect("r2 session should exist");
        assert_eq!(r2.model_name, "deepseek-v4-flash");
        assert_eq!(r2.input_tokens, 19);
        assert_eq!(r2.output_tokens, 4);
        assert_eq!(r2.cache_creation_tokens, 6);
        assert_eq!(r2.cache_read_tokens, 5);
        assert_eq!(r2.total_tokens(), 34);

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn handles_concurrent_interleaved_sessions_without_unknown_model() {
        // Simulates the real-world bug: two RunSSE sessions interleave their
        // log lines. Session r1 starts, then r2 starts before r1's usageTotals
        // arrive. With the old single-pending model, r1's model_name would be
        // lost. The multi-pending HashMap architecture should preserve both.
        let temp_root = std::env::temp_dir().join(format!(
            "token-usage-cursorpp-concurrent-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_root);
        fs::create_dir_all(&temp_root).unwrap();
        let log_path = temp_root.join("Cursor++.concurrent.log");
        fs::write(
            &log_path,
            [
                // r1 starts
                r#"2026-07-14 17:06:17.015 [info] [SVC] AgentService/RunSSE started {"requestId":"r1"}"#,
                r#"2026-07-14 17:06:17.105 [info] [AGENT] → [anthropic/glm-5.2] "main agent" (3 msgs)"#,
                // r1's conversationId appears in an appendMessage line before r2 starts
                r#"2026-07-14 17:06:17.122 [info] [SESSION] appendMessage {"conversationId":"conv-1","keys":["runRequest"],"protoBytes":4599}"#,
                // r2 starts BEFORE r1's usageTotals arrive (subagent)
                r#"2026-07-14 17:06:27.505 [info] [SVC] AgentService/RunSSE started {"requestId":"r2"}"#,
                r#"2026-07-14 17:06:27.608 [info] [AGENT] → [anthropic/glm-5.2] "subagent" (1 msgs)"#,
                // r2's conversationId
                r#"2026-07-14 17:06:27.631 [info] [SESSION] appendMessage {"conversationId":"conv-2","keys":["runRequest"],"protoBytes":81022}"#,
                // r1's usageTotals arrive after r2 has already started
                r#"2026-07-14 17:09:18.638 [info] [SESSION] round checkpoint update {"conversationId":"conv-1","round":0,"usageTotals":{"inputTokens":5000,"outputTokens":100,"cacheReadTokens":200,"cacheWriteTokens":0}}"#,
                // r2's usageTotals
                r#"2026-07-14 17:10:00.000 [info] [SESSION] round checkpoint update {"conversationId":"conv-2","round":0,"usageTotals":{"inputTokens":3000,"outputTokens":50,"cacheReadTokens":100,"cacheWriteTokens":0}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let sessions = read_log_file(&log_path).unwrap();
        assert_eq!(sessions.len(), 2, "should produce 2 sessions, not fewer");

        let r1_session = sessions
            .iter()
            .find(|s| s.session_id == "cursorpp:r1")
            .expect("r1 session should exist");
        assert_eq!(
            r1_session.model_name, "glm-5.2",
            "r1 should have model name, not 'unknown'"
        );

        let r2_session = sessions
            .iter()
            .find(|s| s.session_id == "cursorpp:r2")
            .expect("r2 session should exist");
        assert_eq!(
            r2_session.model_name, "glm-5.2",
            "r2 should have model name, not 'unknown'"
        );

        assert!(
            !sessions.iter().any(|s| s.model_name == "unknown"),
            "no session should have 'unknown' model name"
        );

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn parses_anthropic_prompt_cache_for_model_name() {
        // The [ANTHROPIC] prompt cache line format used by Cursor++ logs
        // when the model provider is Anthropic (e.g. glm-5.2 via Anthropic API).
        let line = r#"2026-07-14 17:00:53.779 [info] [ANTHROPIC] prompt cache {"model":"glm-5.2","cacheRead":68864,"cacheWrite":0,"input":235}"#;
        let model = parse_prompt_cache_model(line).unwrap();
        assert_eq!(model, "glm-5.2");
    }

    #[test]
    fn parses_oai_resp_prompt_cache_still_works() {
        // Regression: the original [OAI_RESP] format should still be matched.
        let line = r#"2026-06-07 13:20:18.662 [info] [OAI_RESP] prompt cache {"model":"gpt-5.5","cached":10,"input":20}"#;
        let model = parse_prompt_cache_model(line).unwrap();
        assert_eq!(model, "gpt-5.5");
    }
}
