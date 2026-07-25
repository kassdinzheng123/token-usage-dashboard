use super::{
    local_hour_from_json, push_timed_text, session_id_of, session_token_hint, ExtractedSession,
    SourceExtract, TimedUserText,
};
use crate::sources::home_dir;
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const CODEX_HOMES_ENV: &str = "TOKEN_USAGE_CODEX_HOMES";

pub fn extract(session_rows: &[Value]) -> Result<SourceExtract, String> {
    let files = collect_session_files()?;
    let mut sessions = Vec::new();

    for row in session_rows {
        let Some(session_id) = session_id_of(row) else {
            continue;
        };
        let token_hint = session_token_hint(row);
        let Some(path) = find_session_file(&files, &session_id) else {
            sessions.push(ExtractedSession {
                session_id,
                project: "General".into(),
                project_key: "codex:general".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        };
        let user_texts = read_user_messages(&path)?;
        let cwd = read_session_cwd(&path)?.unwrap_or_default();
        let (project, project_key) = if cwd.trim().is_empty() {
            ("General".to_string(), "codex:general".to_string())
        } else {
            (
                super::display_project_name(&cwd),
                format!("codex:{cwd}"),
            )
        };
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
        source: "codex".into(),
        sessions,
    })
}

fn read_session_cwd(path: &Path) -> Result<Option<String>, String> {
    let file = File::open(path).map_err(|err| format!("failed to open {}: {err}", path.display()))?;
    let reader = BufReader::new(file);
    for line in reader.lines().take(80) {
        let line = line.map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(cwd) = value
            .pointer("/payload/cwd")
            .or_else(|| value.pointer("/payload/workdir"))
            .or_else(|| value.get("cwd"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Ok(Some(cwd.to_string()));
        }
    }
    Ok(None)
}

fn find_session_file(files: &[PathBuf], session_id: &str) -> Option<PathBuf> {
    let needle = session_id.trim_start_matches('/');
    files.iter().find_map(|path| {
        let stem = path.with_extension("");
        let as_str = stem.to_string_lossy().replace('\\', "/");
        if as_str.ends_with(needle)
            || as_str.ends_with(&format!("/{needle}"))
            || path
                .file_stem()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == needle || needle.ends_with(name))
        {
            Some(path.clone())
        } else if needle.contains('/') {
            let relative = needle.rsplit_once('/').map(|(_, rest)| rest).unwrap_or(needle);
            if as_str.ends_with(relative)
                || path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| relative.ends_with(name) || name == relative)
            {
                Some(path.clone())
            } else {
                None
            }
        } else {
            None
        }
    })
}

fn read_user_messages(path: &Path) -> Result<Vec<TimedUserText>, String> {
    let file = File::open(path).map_err(|err| format!("failed to open {}: {err}", path.display()))?;
    let reader = BufReader::new(file);
    let mut texts = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let Some(payload) = value.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(Value::as_str) != Some("user_message") {
            continue;
        }
        if let Some(message) = payload
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            let hour = value.get("timestamp").and_then(local_hour_from_json);
            push_timed_text(&mut texts, message, hour);
        }
    }
    Ok(texts)
}

fn collect_session_files() -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for home in codex_homes() {
        for subdir in ["sessions", "archived_sessions"] {
            let root = home.join(subdir);
            if root.is_dir() {
                collect_jsonl_files(&root, &mut files)?;
            }
        }
    }
    Ok(files)
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

fn codex_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(value) = std::env::var_os(CODEX_HOMES_ENV) {
        for path in std::env::split_paths(&value) {
            if path.is_dir() {
                homes.push(path);
            }
        }
        if !homes.is_empty() {
            return homes;
        }
    }
    if let Some(path) = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        if path.is_dir() {
            homes.push(path);
            return homes;
        }
    }
    if let Some(home) = home_dir() {
        let path = home.join(".codex");
        if path.is_dir() {
            homes.push(path);
        }
    }
    homes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn extracts_user_messages_only() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-codex-brief-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let session_dir = root.join("sessions").join("2026").join("07").join("15");
        fs::create_dir_all(&session_dir).unwrap();
        let path = session_dir.join("rollout-demo.jsonl");
        let lines = [
            json!({
                "type":"event_msg",
                "payload":{"type":"user_message","message":"检查日志健康度"}
            }),
            json!({
                "type":"event_msg",
                "payload":{"type":"token_count","info":{}}
            }),
            json!({
                "type":"event_msg",
                "payload":{"type":"user_message","message":"继续"}
            }),
        ];
        fs::write(
            &path,
            lines
                .iter()
                .map(|value| serde_json::to_string(value).unwrap())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();

        let previous_home = std::env::var_os("CODEX_HOME");
        let previous_homes = std::env::var_os(CODEX_HOMES_ENV);
        std::env::set_var("CODEX_HOME", &root);
        std::env::remove_var(CODEX_HOMES_ENV);

        let extract = extract(&[json!({
            "sessionId": "2026/07/15/rollout-demo",
            "totalTokens": 10
        })])
        .unwrap();

        match previous_home {
            Some(value) => std::env::set_var("CODEX_HOME", value),
            None => std::env::remove_var("CODEX_HOME"),
        }
        match previous_homes {
            Some(value) => std::env::set_var(CODEX_HOMES_ENV, value),
            None => std::env::remove_var(CODEX_HOMES_ENV),
        }
        let _ = fs::remove_dir_all(&root);

        assert_eq!(extract.sessions.len(), 1);
        assert_eq!(
            extract.sessions[0]
                .user_texts
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            vec!["检查日志健康度", "继续"]
        );
    }
}
