use super::{
    display_project_name, push_capped_text, session_id_of, session_token_hint, ExtractedSession,
    SourceExtract,
};
use crate::sources::opencode::{opencode_db_path, open_opencode_readonly, storage_dir};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
struct OpencodeSessionMeta {
    title: Option<String>,
    directory: Option<String>,
    is_child: bool,
}

/// Extracts opencode sessions for the brief. Modern opencode stores everything
/// in `opencode.db` (session/message/part tables); the session title already
/// arrives on the ledger row as `sessionTitle`, which covers installs where
/// the database is unavailable.
pub fn extract(session_rows: &[Value]) -> Result<SourceExtract, String> {
    let db = storage_dir()
        .as_deref()
        .and_then(opencode_db_path)
        .and_then(|path| open_opencode_readonly(&path).ok());
    let metas = db.as_ref().map(load_session_metas).unwrap_or_default();

    let mut sessions = Vec::new();
    for row in session_rows {
        let Some(session_id) = session_id_of(row) else {
            continue;
        };
        let token_hint = session_token_hint(row);
        let row_title = row
            .get("sessionTitle")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty() && *title != session_id)
            .map(ToString::to_string);

        let Some(meta) = metas.get(&session_id) else {
            // No database row: keep the session usage with its ledger title so
            // the hour timeline can still narrate it.
            sessions.push(ExtractedSession {
                session_id,
                project: "General".into(),
                project_key: "opencode:general".into(),
                usage_only: row_title.is_none(),
                title: row_title,
                user_texts: Vec::new(),
                token_hint,
            });
            continue;
        };

        let title = meta.title.clone().or(row_title);
        let (project, project_key) = match meta.directory.as_deref() {
            Some(directory) => (
                display_project_name(directory),
                format!("opencode:{directory}"),
            ),
            None => ("General".to_string(), "opencode:general".to_string()),
        };

        // Subagent (task) sessions carry usage but their prompts duplicate the
        // parent session's context; keep them out of the narrative.
        if meta.is_child {
            sessions.push(ExtractedSession {
                session_id,
                project,
                project_key,
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        }

        let user_texts = db
            .as_ref()
            .map(|connection| load_user_texts(connection, &session_id))
            .unwrap_or_default();
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
        source: "opencode".into(),
        sessions,
    })
}

fn load_session_metas(connection: &rusqlite::Connection) -> HashMap<String, OpencodeSessionMeta> {
    let Ok(mut statement) = connection.prepare(
        "SELECT id, title, directory, parent_id IS NOT NULL FROM session",
    ) else {
        return HashMap::new();
    };
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, bool>(3)?,
        ))
    });
    let Ok(rows) = rows else {
        return HashMap::new();
    };

    rows.flatten()
        .map(|(id, title, directory, is_child)| {
            let meta = OpencodeSessionMeta {
                title: title
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                directory: directory
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                is_child,
            };
            (id, meta)
        })
        .collect()
}

fn load_user_texts(connection: &rusqlite::Connection, session_id: &str) -> Vec<String> {
    let Ok(mut statement) = connection.prepare(
        r#"
        SELECT p.data
        FROM part p
        JOIN message m ON p.message_id = m.id
        WHERE p.session_id = ?1
          AND json_extract(m.data, '$.role') = 'user'
          AND json_extract(p.data, '$.type') = 'text'
        ORDER BY p.time_created
        "#,
    ) else {
        return Vec::new();
    };
    let rows = statement.query_map([session_id], |row| row.get::<_, String>(0));
    let Ok(rows) = rows else {
        return Vec::new();
    };

    let mut texts = Vec::new();
    for data in rows.flatten() {
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if let Some(text) = value.get("text").and_then(Value::as_str) {
            push_capped_text(&mut texts, text);
        }
    }
    texts
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn falls_back_to_row_title_without_database() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-opencode-brief-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let previous = std::env::var_os("OPENCODE_DATA_DIR");
        std::env::set_var("OPENCODE_DATA_DIR", &root);

        let extract = extract(&[
            json!({"sessionId":"ses_a","sessionTitle":"修复小时摘要","totalTokens":42}),
            json!({"sessionId":"ses_b","totalTokens":7}),
        ])
        .unwrap();

        match previous {
            Some(value) => std::env::set_var("OPENCODE_DATA_DIR", value),
            None => std::env::remove_var("OPENCODE_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(extract.source, "opencode");
        assert_eq!(extract.sessions.len(), 2);
        assert_eq!(
            extract.sessions[0].title.as_deref(),
            Some("修复小时摘要")
        );
        assert!(!extract.sessions[0].usage_only);
        assert!(extract.sessions[1].usage_only);
        assert_eq!(extract.sessions[1].token_hint, 7);
    }
}
