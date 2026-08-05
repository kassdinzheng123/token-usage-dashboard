use super::{
    local_hour_from_rfc3339, push_timed_text, session_id_of, session_token_hint, ExtractedSession,
    SourceExtract, TimedUserText,
};
use crate::sources::grok::discover_sessions_root;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

/// Extracts Grok CLI sessions for the brief. Grok records the user prompt for
/// each turn in `~/.grok/sessions/<url-encoded-project>/prompt_history.jsonl`,
/// keyed by the raw session UUID. The ledger session id is
/// `grok:<uuid>:<loop_index>`, so we strip the prefix/suffix to look up the
/// prompts. There is no per-session title, so the title stays None and the
/// narrative is driven by the user prompts.
pub fn extract(session_rows: &[Value]) -> Result<SourceExtract, String> {
    let Some(root) = discover_sessions_root() else {
        return Ok(SourceExtract {
            source: "grok".into(),
            sessions: Vec::new(),
        });
    };
    let prompts = load_prompt_history(&root);

    let mut sessions = Vec::new();
    for row in session_rows {
        let Some(session_id) = session_id_of(row) else {
            continue;
        };
        let token_hint = session_token_hint(row);
        let Some(uuid) = native_uuid(&session_id) else {
            // Unexpected id shape; keep usage-only.
            sessions.push(ExtractedSession {
                session_id,
                project: "General".into(),
                project_key: "grok:general".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        };
        let Some((project, project_key, texts)) = prompts.get(&uuid) else {
            sessions.push(ExtractedSession {
                session_id,
                project: "General".into(),
                project_key: "grok:general".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        };
        sessions.push(ExtractedSession {
            session_id,
            project: project.clone(),
            project_key: project_key.clone(),
            title: None,
            user_texts: texts.clone(),
            token_hint,
            usage_only: texts.is_empty(),
        });
    }

    Ok(SourceExtract {
        source: "grok".into(),
        sessions,
    })
}

/// `grok:<uuid>:<loop_index>` -> `<uuid>`.
fn native_uuid(session_id: &str) -> Option<String> {
    let rest = session_id.strip_prefix("grok:")?;
    let uuid = rest.split(':').next()?;
    if uuid.is_empty() {
        None
    } else {
        Some(uuid.to_string())
    }
}

/// Scan every `<project>/prompt_history.jsonl` and index prompts by session
/// UUID. Returns uuid -> (project name, project key, user texts).
fn load_prompt_history(
    root: &Path,
) -> BTreeMap<String, (String, String, Vec<TimedUserText>)> {
    let mut by_uuid: BTreeMap<String, (String, String, Vec<TimedUserText>)> = BTreeMap::new();
    let mut files = Vec::new();
    collect_prompt_files(root, &mut files);
    for path in files {
        let project_path = decode_project_root(root, &path);
        let (project, project_key) = project_from_path(&project_path);
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(uuid) = value.get("session_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(text) = value.get("prompt").and_then(Value::as_str) else {
                continue;
            };
            let hour = value.get("timestamp").and_then(Value::as_str).and_then(local_hour_from_rfc3339);
            let entry = by_uuid
                .entry(uuid.to_string())
                .or_insert_with(|| (project.clone(), project_key.clone(), Vec::new()));
            push_timed_text(&mut entry.2, text, hour);
        }
    }
    by_uuid
}

fn collect_prompt_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_prompt_files(&path, files);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("prompt_history.jsonl") {
            files.push(path);
        }
    }
}

/// The `prompt_history.jsonl` lives in `<root>/<url-encoded-absolute-path>/`,
/// e.g. `<root>/%2FUsers%2Fkassdimzheng%2FCodeSpace%2Fmy-pi/prompt_history.jsonl`.
fn decode_project_root(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        .map(percent_decode)
        .unwrap_or_default()
}

fn project_from_path(project_path: &str) -> (String, String) {
    if project_path.trim().is_empty() {
        return ("General".to_string(), "grok:general".to_string());
    }
    let project = super::display_project_name(project_path);
    (project, format!("grok:{project_path}"))
}

/// Minimal percent-decoding for `%XX` escapes (Grok URL-encodes project paths).
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                out.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grok_native_uuid() {
        assert_eq!(
            native_uuid("grok:019fc882-aa89-7563-b2c7-873bf52f0102:8").as_deref(),
            Some("019fc882-aa89-7563-b2c7-873bf52f0102")
        );
        assert_eq!(native_uuid("grok:abc-de:3").as_deref(), Some("abc-de"));
        assert_eq!(native_uuid("not-grok"), None);
    }

    #[test]
    fn percent_decodes_grok_project_path() {
        assert_eq!(
            percent_decode("%2FUsers%2Fkassdimzheng%2FCodeSpace%2Fmy-pi"),
            "/Users/kassdimzheng/CodeSpace/my-pi"
        );
        assert_eq!(percent_decode("my-pi"), "my-pi");
    }

    #[test]
    fn extracts_grok_prompts_from_sessions_root() {
        let root = std::env::temp_dir().join(format!(
            "token-usage-grok-brief-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project_dir = root
            .join("sessions")
            .join("%2FUsers%2Fkassdimzheng%2FCodeSpace%2Fmy-pi");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("prompt_history.jsonl"),
            r#"{"timestamp":"2026-08-04T01:00:00Z","session_id":"019fc882-aa89-7563-b2c7-873bf52f0102","prompt":"优化请求计数逻辑","is_bash":false}
{"timestamp":"2026-08-04T02:30:00Z","session_id":"019fc882-aa89-7563-b2c7-873bf52f0102","prompt":"补充测试用例","is_bash":false}
{"timestamp":"2026-08-04T03:00:00Z","session_id":"other-uuid","prompt":"别的会话","is_bash":false}
"#,
        )
        .unwrap();

        let previous = std::env::var_os("GROK_HOME");
        std::env::set_var("GROK_HOME", &root);
        let rows = vec![serde_json::json!({
            "sessionId": "grok:019fc882-aa89-7563-b2c7-873bf52f0102:8",
            "totalTokens": 60781,
            "inputTokens": 1756,
            "outputTokens": 1041,
        })];
        let extract = extract(&rows).unwrap();
        match previous {
            Some(value) => std::env::set_var("GROK_HOME", value),
            None => std::env::remove_var("GROK_HOME"),
        }
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(extract.source, "grok");
        assert_eq!(extract.sessions.len(), 1);
        let session = &extract.sessions[0];
        assert_eq!(session.session_id, "grok:019fc882-aa89-7563-b2c7-873bf52f0102:8");
        assert!(!session.usage_only);
        assert_eq!(session.token_hint, 60781);
        assert_eq!(session.project, "my-pi");
        assert_eq!(
            session.project_key,
            "grok:/Users/kassdimzheng/CodeSpace/my-pi"
        );
        assert_eq!(session.user_texts.len(), 2);
        assert_eq!(session.user_texts[0].text, "优化请求计数逻辑");
        assert_eq!(session.user_texts[1].text, "补充测试用例");
    }
}