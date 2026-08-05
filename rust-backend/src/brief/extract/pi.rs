use super::{
    local_hour_from_json, push_timed_text, session_id_of, session_token_hint, ExtractedSession,
    SourceExtract, TimedUserText,
};
use crate::sources::pi::{collect_jsonl_files, discover_sessions_dirs, extract_project_path};
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

/// Extracts Pi-Agent sessions for the brief. Pi stores one JSONL per session
/// under `~/.pi/agent/sessions/<encoded-project>/<timestamp>_<session_id>.jsonl`.
/// User messages carry the actual prompt text; there is no per-session title
/// file, so the title is derived from the first user message when available.
pub fn extract(session_rows: &[Value]) -> Result<SourceExtract, String> {
    let mut sessions = Vec::new();
    for row in session_rows {
        let Some(session_id) = session_id_of(row) else {
            continue;
        };
        let token_hint = session_token_hint(row);
        let Some((path, project_path)) = find_session_file(&session_id) else {
            sessions.push(ExtractedSession {
                session_id,
                project: "General".into(),
                project_key: "pi:general".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        };
        let (cwd, user_texts) = read_session_file(&path)?;
        let (project, project_key) = project_from_path(project_path.as_ref(), cwd.as_deref());
        let usage_only = user_texts.is_empty();
        sessions.push(ExtractedSession {
            session_id,
            project,
            project_key,
            title: None,
            user_texts,
            token_hint,
            usage_only,
        });
    }

    Ok(SourceExtract {
        source: "pi".into(),
        sessions,
    })
}

fn project_from_path(project_path: &str, cwd: Option<&str>) -> (String, String) {
    // Prefer the real cwd recorded in the session file (reliable for dirs
    // containing hyphens); fall back to the encoded project path (e.g.
    // `--Users-...-my-pi--`) when it is absent.
    let (project, key) = match cwd.map(str::trim).filter(|value| !value.is_empty()) {
        Some(cwd) => (super::display_project_name(cwd), cwd.to_string()),
        None if project_path.trim().is_empty() || project_path == "unknown" => {
            return ("General".to_string(), "pi:general".to_string());
        }
        None => (super::decode_claude_project_dir(project_path), project_path.to_string()),
    };
    (project, format!("pi:{key}"))
}

fn find_session_file(session_id: &str) -> Option<(PathBuf, String)> {
    let needle = format!("_{session_id}.jsonl");
    for sessions_dir in discover_sessions_dirs() {
        let mut files = Vec::new();
        if collect_jsonl_files(&sessions_dir, &mut files).is_err() {
            continue;
        }
        for path in files {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if !file_name.ends_with(&needle) {
                continue;
            }
            let project_path = extract_project_path(&sessions_dir, &path);
            return Some((path, project_path));
        }
    }
    None
}

fn read_session_file(path: &Path) -> Result<(Option<String>, Vec<TimedUserText>), String> {
    let file = File::open(path).map_err(|err| format!("failed to open {}: {err}", path.display()))?;
    let reader = BufReader::new(file);
    let mut cwd = None;
    let mut user_texts = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session") => {
                if cwd.is_none() {
                    cwd = value
                        .get("cwd")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(ToString::to_string);
                }
            }
            Some("message") => {
                let message = value.get("message").cloned().unwrap_or_default();
                if message.get("role").and_then(Value::as_str) != Some("user") {
                    continue;
                }
                if let Some(text) = extract_user_text(&message) {
                    let hour = value.get("timestamp").and_then(local_hour_from_json);
                    push_timed_text(&mut user_texts, &text, hour);
                }
            }
            _ => {}
        }
    }

    Ok((cwd, user_texts))
}

fn extract_user_text(message: &Value) -> Option<String> {
    let content = message.get("content")?;
    let text = match content {
        Value::String(value) => value.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(value) = item.get("text").and_then(Value::as_str) {
                        let trimmed = value.trim();
                        if !trimmed.is_empty() {
                            parts.push(trimmed.to_string());
                        }
                    }
                }
            }
            if parts.is_empty() {
                return None;
            }
            parts.join("\n")
        }
        _ => return None,
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_user_text_from_pi_message() {
        let message = json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "  优化上下文计数  "},
                {"type": "image", "data": "base64"}
            ]
        });
        assert_eq!(
            extract_user_text(&message).as_deref(),
            Some("优化上下文计数")
        );
    }

    #[test]
    fn extracts_user_text_from_string_content() {
        let message = json!({"role": "user", "content": "直接提问"});
        assert_eq!(extract_user_text(&message).as_deref(), Some("直接提问"));
    }

    #[test]
    fn extracts_pi_session_from_agent_dir() {
        let root = std::env::temp_dir().join(format!(
            "token-usage-pi-brief-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project_dir = root.join("--Users-kassdimzheng-CodeSpace-my-pi--");
        std::fs::create_dir_all(&project_dir).unwrap();
        let session_id = "019fc1c4-8946-77b7-9d5c-cff526b54d1c";
        let file = project_dir.join(format!("2026-08-02T09-18-30-726Z_{session_id}.jsonl"));
        std::fs::write(
            &file,
            r#"{"type":"session","version":3,"id":"019fc1c4-8946-77b7-9d5c-cff526b54d1c","timestamp":"2026-08-02T09:18:30.726Z","cwd":"/Users/kassdinzheng/CodeSpace/my-pi"}
{"type":"message","timestamp":"2026-08-02T09:18:32.042Z","message":{"role":"user","content":[{"type":"text","text":"优化上下文计数系统"}]}}
{"type":"message","timestamp":"2026-08-02T09:19:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"好的"}]}}
"#,
        )
        .unwrap();

        let previous = std::env::var_os("PI_AGENT_DIR");
        std::env::set_var("PI_AGENT_DIR", &root);
        let rows = vec![json!({
            "sessionId": session_id,
            "totalTokens": 1000,
            "inputTokens": 500,
            "outputTokens": 500,
        })];
        let extract = extract(&rows).unwrap();
        match previous {
            Some(value) => std::env::set_var("PI_AGENT_DIR", value),
            None => std::env::remove_var("PI_AGENT_DIR"),
        }
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(extract.source, "pi");
        assert_eq!(extract.sessions.len(), 1);
        let session = &extract.sessions[0];
        assert_eq!(session.session_id, session_id);
        assert!(!session.usage_only);
        assert_eq!(session.token_hint, 1000);
        assert_eq!(session.project, "my-pi");
        assert_eq!(
            session.project_key,
            "pi:/Users/kassdinzheng/CodeSpace/my-pi"
        );
        assert_eq!(session.user_texts.len(), 1);
        assert_eq!(session.user_texts[0].text, "优化上下文计数系统");
        assert!(session.user_texts[0].hour.is_some());
    }

    #[test]
    fn falls_back_to_usage_only_when_file_missing() {
        let root = std::env::temp_dir().join(format!(
            "token-usage-pi-brief-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let previous = std::env::var_os("PI_AGENT_DIR");
        std::env::set_var("PI_AGENT_DIR", &root);
        let rows = vec![json!({
            "sessionId": "missing-session-id",
            "totalTokens": 42,
        })];
        let extract = extract(&rows).unwrap();
        match previous {
            Some(value) => std::env::set_var("PI_AGENT_DIR", value),
            None => std::env::remove_var("PI_AGENT_DIR"),
        }
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(extract.sessions.len(), 1);
        assert!(extract.sessions[0].usage_only);
        assert_eq!(extract.sessions[0].token_hint, 42);
        assert_eq!(extract.sessions[0].project, "General");
    }
}