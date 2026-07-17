use super::{
    push_capped_text, session_id_of, session_token_hint, ExtractedSession, SourceExtract,
};
use crate::sources::home_dir;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const KIMI_CODE_HOME_ENV: &str = "KIMI_CODE_HOME";
const KIMI_WORK_HOME_ENV: &str = "KIMI_WORK_HOME";
const KIMI_WORK_RELATIVE_HOME: &str =
    "Library/Application Support/kimi-desktop/daimon-share/daimon/runtime/kimi-code/home";

pub fn extract(session_rows: &[Value]) -> Result<SourceExtract, String> {
    let mut sessions = Vec::new();

    for row in session_rows {
        let Some(session_id) = session_id_of(row) else {
            continue;
        };
        let token_hint = session_token_hint(row);
        let Some((kind, native_id)) = native_session_id(&session_id) else {
            continue;
        };

        // Auto title-generation sessions carry real usage but no user dialog.
        if native_id.starts_with("ctitle-") {
            sessions.push(ExtractedSession {
                session_id,
                project: "General".into(),
                project_key: "kimi:general".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        }

        let Some(dir) = kimi_root(kind).and_then(|root| find_session_dir(&root, native_id))
        else {
            sessions.push(ExtractedSession {
                session_id,
                project: "General".into(),
                project_key: "kimi:general".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        };

        let state = read_state(&dir);
        let title = state.as_ref().and_then(|state| {
            state
                .get("title")
                .and_then(Value::as_str)
                .and_then(clean_title)
        });
        let directory = state.as_ref().and_then(|state| {
            ["cwd", "workDir"].iter().find_map(|key| {
                state
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
            })
        });
        let (project, project_key) = match directory {
            Some(dir) => (
                super::display_project_name(dir),
                format!("kimi:{dir}"),
            ),
            None => ("General".to_string(), "kimi:general".to_string()),
        };
        let user_texts = read_user_texts(&dir);
        let usage_only = title.is_none() && user_texts.is_empty();
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
        source: "kimi".into(),
        sessions,
    })
}

fn native_session_id(session_id: &str) -> Option<(&str, &str)> {
    let (kind, native) = session_id.split_once(':')?;
    if !matches!(kind, "kimi-code" | "kimi-work") || native.trim().is_empty() {
        return None;
    }
    Some((kind, native))
}

fn kimi_root(kind: &str) -> Option<PathBuf> {
    match kind {
        "kimi-code" => std::env::var_os(KIMI_CODE_HOME_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|home| home.join(".kimi-code"))),
        "kimi-work" => std::env::var_os(KIMI_WORK_HOME_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|home| home.join(KIMI_WORK_RELATIVE_HOME))),
        _ => None,
    }
    .filter(|path| path.is_dir())
}

fn find_session_dir(root: &Path, native_id: &str) -> Option<PathBuf> {
    let workspaces = fs::read_dir(root.join("sessions")).ok()?;
    for workspace in workspaces.flatten() {
        let candidate = workspace.path().join(native_id);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn read_state(dir: &Path) -> Option<Value> {
    let text = fs::read_to_string(dir.join("state.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Kimi Work titles may carry a leading `<meta ... />` annotation; drop it.
fn clean_title(title: &str) -> Option<String> {
    let mut text = title.trim();
    if let Some(rest) = text.strip_prefix("<meta") {
        if let Some(end) = rest.find('>') {
            text = rest[end + 1..].trim();
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn read_user_texts(dir: &Path) -> Vec<String> {
    let mut prompt_texts = Vec::new();
    let mut message_texts = Vec::new();
    let Ok(agents) = fs::read_dir(dir.join("agents")) else {
        return prompt_texts;
    };
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
                Some("turn.prompt") => {
                    if value.pointer("/origin/kind").and_then(Value::as_str) != Some("user") {
                        continue;
                    }
                    if let Some(input) = value.get("input").and_then(Value::as_array) {
                        for part in input {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                push_capped_text(&mut prompt_texts, text);
                            }
                        }
                    }
                }
                Some("context.append_message") => {
                    let Some(message) = value.get("message") else {
                        continue;
                    };
                    if message.get("role").and_then(Value::as_str) != Some("user") {
                        continue;
                    }
                    if message.pointer("/origin/kind").and_then(Value::as_str) != Some("user")
                    {
                        continue;
                    }
                    if let Some(content) = message.get("content").and_then(Value::as_array) {
                        for part in content {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                push_capped_text(&mut message_texts, text);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // turn.prompt and context.append_message duplicate the same user input;
    // prefer turn.prompt and only fall back when it is absent.
    if prompt_texts.is_empty() {
        message_texts
    } else {
        prompt_texts
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
    fn strips_meta_prefix_from_title() {
        assert_eq!(
            clean_title("<meta awareness=\"low\" timestamp=\"2026-07-17 16:29\" /> 新实例已置顶"),
            Some("新实例已置顶".to_string())
        );
        assert_eq!(clean_title(" 普通标题 "), Some("普通标题".to_string()));
        assert_eq!(clean_title("<meta x=\"1\" />"), None);
    }

    #[test]
    fn extracts_title_project_and_user_texts() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-kimi-brief-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let session_dir = root
            .join("sessions")
            .join("wd_demo_0123456789ab")
            .join("conv-abc123");
        fs::create_dir_all(session_dir.join("agents").join("main")).unwrap();
        fs::write(
            session_dir.join("state.json"),
            serde_json::to_string(&json!({
                "id": "conv-abc123",
                "workDir": "/Users/demo/token-usage",
                "createdAt": "2026-07-17T08:29:45.466Z",
                "title": "<meta awareness=\"low\" /> 修复小时摘要",
            }))
            .unwrap(),
        )
        .unwrap();
        let lines = [
            json!({"type":"turn.prompt","input":[{"type":"text","text":"帮我修复小时摘要"}],"origin":{"kind":"user"},"time":1784280997000i64}),
            json!({"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"帮我修复小时摘要"}],"origin":{"kind":"user"}},"time":1784280997000i64}),
            json!({"type":"turn.prompt","input":[{"type":"text","text":"子代理指令"}],"origin":{"kind":"system_trigger"},"time":1784280998000i64}),
        ];
        fs::write(
            session_dir.join("agents").join("main").join("wire.jsonl"),
            lines
                .iter()
                .map(|value| serde_json::to_string(value).unwrap())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();

        let previous_code = std::env::var_os(KIMI_CODE_HOME_ENV);
        let previous_work = std::env::var_os(KIMI_WORK_HOME_ENV);
        std::env::set_var(KIMI_CODE_HOME_ENV, &root);
        std::env::set_var(KIMI_WORK_HOME_ENV, root.join("missing"));

        let extract = extract(&[
            json!({"sessionId":"kimi-code:conv-abc123","totalTokens":42}),
            json!({"sessionId":"kimi-code:ctitle-019f6f32","totalTokens":7}),
        ])
        .unwrap();

        restore_env(KIMI_CODE_HOME_ENV, previous_code);
        restore_env(KIMI_WORK_HOME_ENV, previous_work);
        let _ = fs::remove_dir_all(&root);

        assert_eq!(extract.sessions.len(), 2);
        let session = &extract.sessions[0];
        assert_eq!(session.title.as_deref(), Some("修复小时摘要"));
        assert_eq!(session.project, "token-usage");
        assert_eq!(session.project_key, "kimi:/Users/demo/token-usage");
        assert_eq!(session.user_texts, vec!["帮我修复小时摘要"]);
        assert!(!session.usage_only);
        assert!(extract.sessions[1].usage_only);
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
