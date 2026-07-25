use super::{
    local_hour_from_millis, push_timed_text, session_id_of, session_token_hint, truncate_chars,
    ExtractedSession, SourceExtract, TimedUserText, MAX_USER_CHARS, MAX_USER_MESSAGES,
};
use crate::sources::home_dir;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

const DATA_DIR_ENV: &str = "ZCODE_DATA_DIR";
const ALT_DATA_DIR_ENV: &str = "TOKEN_USAGE_ZCODE_DATA_DIR";
const DB_RELATIVE_PATH: &str = "cli/db/db.sqlite";

pub fn extract(session_rows: &[Value]) -> Result<SourceExtract, String> {
    let mut by_native: BTreeMap<String, (i64, Vec<String>)> = BTreeMap::new();
    for row in session_rows {
        let Some(session_id) = session_id_of(row) else {
            continue;
        };
        let Some(native_id) = native_session_id(&session_id) else {
            continue;
        };
        let token_hint = session_token_hint(row);
        let entry = by_native.entry(native_id).or_insert_with(|| (0, Vec::new()));
        entry.0 = entry.0.saturating_add(token_hint);
        entry.1.push(session_id);
    }

    let Some(db_path) = discover_db_path() else {
        let sessions = by_native
            .into_iter()
            .map(|(native_id, (token_hint, ids))| ExtractedSession {
                session_id: ids.first().cloned().unwrap_or(native_id),
                project: "General".into(),
                project_key: "zcode:general".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            })
            .collect();
        return Ok(SourceExtract {
            source: "zcode".into(),
            sessions,
        });
    };

    let connection = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|err| format!("failed to open {}: {err}", db_path.display()))?;

    let mut sessions = Vec::new();
    for (native_id, (token_hint, ids)) in by_native {
        let title = load_title(&connection, &native_id)?;
        let directory = load_directory(&connection, &native_id)?;
        let (project, project_key) = match directory.as_deref() {
            Some(dir) if !dir.trim().is_empty() => (
                super::display_project_name(dir),
                format!("zcode:{dir}"),
            ),
            _ => ("General".to_string(), "zcode:general".to_string()),
        };
        let mut user_texts = load_user_parts(&connection, &native_id)?;
        if user_texts.is_empty() {
            user_texts = load_input_history(&connection, &native_id)?;
        }
        let usage_only = title.is_none() && user_texts.is_empty();
        sessions.push(ExtractedSession {
            session_id: ids.first().cloned().unwrap_or_else(|| native_id.clone()),
            project,
            project_key,
            title,
            user_texts,
            token_hint,
            usage_only,
        });
    }

    Ok(SourceExtract {
        source: "zcode".into(),
        sessions,
    })
}

fn load_directory(connection: &Connection, native_id: &str) -> Result<Option<String>, String> {
    let mut statement = match connection.prepare("SELECT directory FROM session WHERE id = ?1 LIMIT 1") {
        Ok(statement) => statement,
        Err(_) => return Ok(None),
    };
    let directory = statement
        .query_row([native_id], |row| row.get::<_, String>(0))
        .optional()
        .map_err(|err| err.to_string())?;
    Ok(directory
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty()))
}

pub fn native_session_id(session_id: &str) -> Option<String> {
    let rest = session_id.strip_prefix("zcode:")?;
    let native = rest.split(':').next()?.trim();
    if native.is_empty() {
        None
    } else {
        Some(native.to_string())
    }
}

fn load_title(connection: &Connection, native_id: &str) -> Result<Option<String>, String> {
    let mut statement = connection
        .prepare("SELECT title FROM session WHERE id = ?1 LIMIT 1")
        .map_err(|err| err.to_string())?;
    let title = statement
        .query_row([native_id], |row| row.get::<_, String>(0))
        .optional()
        .map_err(|err| err.to_string())?;
    Ok(title
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty()))
}

fn load_user_parts(connection: &Connection, native_id: &str) -> Result<Vec<TimedUserText>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT p.data, m.data, p.time_created
            FROM part p
            JOIN message m ON m.id = p.message_id
            WHERE p.session_id = ?1
            ORDER BY p.time_created ASC, p.id ASC
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([native_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .map_err(|err| err.to_string())?;

    let mut texts = Vec::new();
    for row in rows {
        let (part_data, message_data, time_created) = row.map_err(|err| err.to_string())?;
        let Ok(message) = serde_json::from_str::<Value>(&message_data) else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Ok(part) = serde_json::from_str::<Value>(&part_data) else {
            continue;
        };
        if part.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        if let Some(text) = part
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            let hour = time_created.and_then(local_hour_from_millis);
            push_timed_text(&mut texts, text, hour);
        }
    }
    Ok(texts)
}

fn load_input_history(connection: &Connection, native_id: &str) -> Result<Vec<TimedUserText>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT text, time_created
            FROM input_history
            WHERE session_id = ?1
            ORDER BY time_created ASC, id ASC
            LIMIT ?2
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params![native_id, (MAX_USER_MESSAGES * 4) as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .map_err(|err| err.to_string())?;

    let mut texts = Vec::new();
    for row in rows {
        let (text, time_created) = row.map_err(|err| err.to_string())?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        texts.push(TimedUserText {
            text: truncate_chars(trimmed, MAX_USER_CHARS),
            hour: time_created.and_then(local_hour_from_millis),
        });
    }
    Ok(texts)
}

trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

fn discover_db_path() -> Option<PathBuf> {
    for env_name in [DATA_DIR_ENV, ALT_DATA_DIR_ENV] {
        if let Ok(path) = std::env::var(env_name) {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
            let nested = path.join(DB_RELATIVE_PATH);
            if nested.is_file() {
                return Some(nested);
            }
            let direct = path.join("db.sqlite");
            if direct.is_file() {
                return Some(direct);
            }
        }
    }
    let home = home_dir()?;
    let path = home.join(".zcode").join(DB_RELATIVE_PATH);
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn parses_native_session_id() {
        assert_eq!(
            native_session_id("zcode:sess_1:usage-9").as_deref(),
            Some("sess_1")
        );
    }

    #[test]
    fn dedupes_by_native_session_and_loads_title_plus_user_text() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "token-usage-zcode-brief-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("db.sqlite");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE session (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL
                );
                CREATE TABLE message (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    data TEXT NOT NULL
                );
                CREATE TABLE part (
                    id TEXT PRIMARY KEY,
                    message_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL,
                    data TEXT NOT NULL
                );
                CREATE TABLE input_history (
                    id TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL,
                    session_id TEXT,
                    text TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    time_created INTEGER NOT NULL
                );
                INSERT INTO session (id, title) VALUES ('sess_1', '模型选择器问题');
                INSERT INTO message (id, session_id, data)
                VALUES ('msg_1', 'sess_1', '{"role":"user"}');
                INSERT INTO part (id, message_id, session_id, time_created, data)
                VALUES ('part_1', 'msg_1', 'sess_1', 1, '{"type":"text","text":"为什么模型选择器不记住？"}');
                INSERT INTO input_history (id, project_id, session_id, text, kind, time_created)
                VALUES ('ih_1', 'proj', 'sess_1', '补充一句', 'prompt', 2);
                "#,
            )
            .unwrap();

        let previous = std::env::var_os(ALT_DATA_DIR_ENV);
        std::env::set_var(ALT_DATA_DIR_ENV, &db_path);

        let extract = extract(&[
            json!({"sessionId":"zcode:sess_1:u1","totalTokens":10}),
            json!({"sessionId":"zcode:sess_1:u2","totalTokens":5}),
        ])
        .unwrap();

        match previous {
            Some(value) => std::env::set_var(ALT_DATA_DIR_ENV, value),
            None => std::env::remove_var(ALT_DATA_DIR_ENV),
        }
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(extract.sessions.len(), 1);
        assert_eq!(extract.sessions[0].title.as_deref(), Some("模型选择器问题"));
        assert_eq!(extract.sessions[0].token_hint, 15);
        assert!(extract.sessions[0]
            .user_texts
            .iter()
            .any(|entry| entry.text.contains("模型选择器")));
    }
}
