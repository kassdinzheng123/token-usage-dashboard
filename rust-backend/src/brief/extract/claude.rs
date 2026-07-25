use super::{
    local_hour_from_json, push_timed_text, session_id_of, session_token_hint, ExtractedSession,
    SourceExtract, TimedUserText,
};
use crate::sources::home_dir;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
const PROJECTS_DIR: &str = "projects";

pub fn extract(session_rows: &[Value]) -> Result<SourceExtract, String> {
    let mut sessions = Vec::new();
    for row in session_rows {
        let Some(session_id) = session_id_of(row) else {
            continue;
        };
        let token_hint = session_token_hint(row);
        let Some(path) = find_session_file(&session_id)? else {
            sessions.push(ExtractedSession {
                session_id,
                project: "General".into(),
                project_key: "claude:general".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        };
        let (title, user_texts) = read_session_file(&path)?;
        let usage_only = title.is_none() && user_texts.is_empty();
        let (project, project_key) = project_from_session_path(&path);
        sessions.push(ExtractedSession {
            session_id,
            project,
            project_key,
            title,
            user_texts,
            token_hint,
            usage_only,
        });
    }

    Ok(SourceExtract {
        source: "claude".into(),
        sessions,
    })
}

fn project_from_session_path(path: &Path) -> (String, String) {
    // .../<projects>/<encoded-project>/<session>.jsonl
    let encoded = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("general");
    let project = super::decode_claude_project_dir(encoded);
    let project_key = format!("claude:{encoded}");
    (project, project_key)
}

fn find_session_file(session_id: &str) -> Result<Option<PathBuf>, String> {
    let needle = format!("{session_id}.jsonl");
    for projects_dir in discover_projects_dirs() {
        let mut files = Vec::new();
        collect_jsonl_files(&projects_dir, &mut files)?;
        if let Some(path) = files.into_iter().find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == needle)
        }) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn read_session_file(path: &Path) -> Result<(Option<String>, Vec<TimedUserText>), String> {
    let file = File::open(path).map_err(|err| format!("failed to open {}: {err}", path.display()))?;
    let reader = BufReader::new(file);
    let mut title = None;
    let mut user_texts = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("custom-title") => {
                if let Some(text) = value
                    .get("customTitle")
                    .or_else(|| value.get("title"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    title = Some(text.to_string());
                }
            }
            Some("ai-title") => {
                if title.is_none() {
                    if let Some(text) = value
                        .get("aiTitle")
                        .or_else(|| value.get("title"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        title = Some(text.to_string());
                    }
                }
            }
            Some("user") => {
                if let Some(text) = extract_user_text(&value) {
                    let hour = value.get("timestamp").and_then(local_hour_from_json);
                    push_timed_text(&mut user_texts, &text, hour);
                }
            }
            _ => {}
        }
    }

    Ok((title, user_texts))
}

fn extract_user_text(value: &Value) -> Option<String> {
    let message = value.get("message")?;
    let content = message.get("content")?;
    match content {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("tool_result") {
                    continue;
                }
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed);
                    }
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

fn discover_projects_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = BTreeSet::new();

    if let Some(raw) = std::env::var_os(CLAUDE_CONFIG_DIR) {
        for config_dir in split_env_paths(&raw.to_string_lossy()) {
            let path = PathBuf::from(config_dir);
            if path.file_name().and_then(|name| name.to_str()) == Some(PROJECTS_DIR) {
                push_existing_dir(&mut dirs, &mut seen, path);
            } else {
                push_existing_dir(&mut dirs, &mut seen, path.join(PROJECTS_DIR));
            }
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
    push_existing_dir(&mut dirs, &mut seen, home.join(".claude").join(PROJECTS_DIR));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn extracts_title_and_skips_tool_results() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-claude-brief-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("projects").join("demo");
        fs::create_dir_all(&project).unwrap();
        let session_id = "sess-brief-1";
        let path = project.join(format!("{session_id}.jsonl"));
        let lines = [
            json!({"type":"custom-title","customTitle":"磁盘清理","sessionId":session_id}),
            json!({"type":"user","message":{"content":[{"type":"text","text":"帮我清理磁盘"}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","content":"ls output"}]}}),
            json!({"type":"user","message":{"content":[{"type":"text","text":"继续"}]}}),
        ];
        let body = lines
            .iter()
            .map(|value| serde_json::to_string(value).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, body).unwrap();

        let previous = std::env::var_os(CLAUDE_CONFIG_DIR);
        std::env::set_var(CLAUDE_CONFIG_DIR, &root);

        let extract = extract(&[json!({
            "sessionId": session_id,
            "totalTokens": 42
        })])
        .unwrap();

        match previous {
            Some(value) => std::env::set_var(CLAUDE_CONFIG_DIR, value),
            None => std::env::remove_var(CLAUDE_CONFIG_DIR),
        }
        let _ = fs::remove_dir_all(&root);

        assert_eq!(extract.sessions.len(), 1);
        assert_eq!(extract.sessions[0].title.as_deref(), Some("磁盘清理"));
        assert_eq!(
            extract.sessions[0]
                .user_texts
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            vec!["帮我清理磁盘", "继续"]
        );
        assert!(!extract.sessions[0].usage_only);
        assert_eq!(extract.coverage(), "full");
    }

    #[test]
    fn stamps_local_hour_from_user_timestamp() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-claude-brief-hour-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("projects").join("demo");
        fs::create_dir_all(&project).unwrap();
        let session_id = "sess-brief-hour";
        let path = project.join(format!("{session_id}.jsonl"));
        // Fixed offset so the hour assertion is timezone-stable: 06:30 and 11:15 local+00.
        let lines = [
            json!({"type":"user","timestamp":"2026-07-21T06:30:00+00:00","message":{"content":"早上做交接"}}),
            json!({"type":"user","timestamp":"2026-07-21T11:15:00+00:00","message":{"content":"上午验因果链"}}),
        ];
        let body = lines
            .iter()
            .map(|value| serde_json::to_string(value).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, body).unwrap();

        let previous = std::env::var_os(CLAUDE_CONFIG_DIR);
        std::env::set_var(CLAUDE_CONFIG_DIR, &root);

        let extract = extract(&[json!({
            "sessionId": session_id,
            "totalTokens": 42
        })])
        .unwrap();

        match previous {
            Some(value) => std::env::set_var(CLAUDE_CONFIG_DIR, value),
            None => std::env::remove_var(CLAUDE_CONFIG_DIR),
        }
        let _ = fs::remove_dir_all(&root);

        let texts = &extract.sessions[0].user_texts;
        assert_eq!(texts.len(), 2);
        assert_eq!(texts[0].text, "早上做交接");
        assert_eq!(texts[1].text, "上午验因果链");
        assert_eq!(
            texts[0].hour,
            super::super::local_hour_from_rfc3339("2026-07-21T06:30:00+00:00")
        );
        assert_eq!(
            texts[1].hour,
            super::super::local_hour_from_rfc3339("2026-07-21T11:15:00+00:00")
        );
    }
}
