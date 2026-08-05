use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, unix_millis_to_utc_parts, LocalSession, SourceError};
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;

const DATA_DIR_ENV: &str = "ZCODE_DATA_DIR";
const ALT_DATA_DIR_ENV: &str = "TOKEN_USAGE_ZCODE_DATA_DIR";
const DB_RELATIVE_PATH: &str = "cli/db/db.sqlite";
const UNKNOWN_MODEL: &str = "unknown";

pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let Some(db_path) = discover_db_path() else {
        return Ok(Vec::new());
    };

    if !db_path.is_file() {
        return Ok(Vec::new());
    }

    let connection =
        Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        r#"
        SELECT
            id,
            session_id,
            model_id,
            input_tokens,
            output_tokens,
            reasoning_tokens,
            cache_creation_input_tokens,
            cache_read_input_tokens,
            COALESCE(completed_at, started_at) AS event_at
        FROM model_usage
        WHERE COALESCE(input_tokens, 0) > 0
           OR COALESCE(output_tokens, 0) > 0
           OR COALESCE(reasoning_tokens, 0) > 0
           OR COALESCE(cache_creation_input_tokens, 0) > 0
           OR COALESCE(cache_read_input_tokens, 0) > 0
        ORDER BY event_at
        "#,
    )?;

    let rows = statement.query_map([], |row| {
        Ok(UsageRow {
            id: row.get(0)?,
            session_id: row.get(1)?,
            model_id: row.get::<_, Option<String>>(2)?,
            input_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
            output_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
            reasoning_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or_default(),
            cache_creation_tokens: row.get::<_, Option<i64>>(6)?.unwrap_or_default(),
            cache_read_tokens: row.get::<_, Option<i64>>(7)?.unwrap_or_default(),
            event_at: row.get::<_, Option<i64>>(8)?.unwrap_or_default(),
        })
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        let row = row?;
        if let Some(session) = usage_to_session(&row) {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

#[derive(Debug)]
struct UsageRow {
    id: String,
    session_id: String,
    model_id: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    reasoning_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    event_at: i64,
}

fn usage_to_session(row: &UsageRow) -> Option<LocalSession> {
    // ZCode stores OpenAI-style totals: input_tokens already includes cache
    // read/write. Split them out so fresh input and cache bill separately.
    let cache_creation_tokens = row.cache_creation_tokens.max(0);
    let cache_read_tokens = row.cache_read_tokens.max(0);
    let input_tokens = row
        .input_tokens
        .saturating_sub(cache_read_tokens)
        .saturating_sub(cache_creation_tokens)
        .max(0);
    let output_tokens = row
        .output_tokens
        .saturating_add(row.reasoning_tokens)
        .max(0);

    let total_tokens =
        input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens;
    if total_tokens <= 0 {
        return None;
    }

    let (date, time) = unix_millis_to_utc_parts(row.event_at)?;
    let model_name = row
        .model_id
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(UNKNOWN_MODEL)
        .to_string();
    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    };

    Some(LocalSession {
        session_id: format!("zcode:{}:{}", row.session_id, row.id),
        date,
        time,
        model_name: model_name.clone(),
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_tokens_override: None,
        total_cost: model_cost_usd(&model_name, usage),
        model_breakdowns: Vec::new(),
    })
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
            if path.is_dir() {
                let direct = path.join("db.sqlite");
                if direct.is_file() {
                    return Some(direct);
                }
            }
        }
    }

    let home = home_dir()?;
    let path = home.join(".zcode").join(DB_RELATIVE_PATH);
    if path.is_file() {
        return Some(path);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn splits_cached_tokens_from_openai_style_input() {
        let session = usage_to_session(&UsageRow {
            id: "usage-1".into(),
            session_id: "sess_1".into(),
            model_id: Some("GLM-5.2".into()),
            input_tokens: 100,
            output_tokens: 20,
            reasoning_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 64,
            event_at: 1_700_000_000_000,
        })
        .unwrap();

        assert_eq!(session.input_tokens, 36);
        assert_eq!(session.output_tokens, 25);
        assert_eq!(session.cache_read_tokens, 64);
        assert_eq!(session.cache_creation_tokens, 0);
        assert_eq!(session.model_name, "GLM-5.2");
        assert!(session.session_id.starts_with("zcode:sess_1:"));
    }

    #[test]
    fn loads_usage_rows_from_sqlite() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "token-usage-zcode-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db_dir = dir.join("cli").join("db");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db_path = db_dir.join("db.sqlite");
        seed_db(&db_path);

        let previous = std::env::var_os(DATA_DIR_ENV);
        std::env::set_var(DATA_DIR_ENV, &dir);
        let sessions = load_sessions().unwrap();
        restore_env(previous);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].input_tokens, 36);
        assert_eq!(sessions[0].output_tokens, 20);
        assert_eq!(sessions[0].cache_read_tokens, 64);
        assert_eq!(sessions[0].model_name, "GLM-5.2");
    }

    fn seed_db(db_path: &std::path::Path) {
        let connection = Connection::open(db_path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE session (
                    id TEXT PRIMARY KEY,
                    directory TEXT NOT NULL
                );
                CREATE TABLE model_usage (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    input_tokens INTEGER NOT NULL DEFAULT 0,
                    output_tokens INTEGER NOT NULL DEFAULT 0,
                    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
                    started_at INTEGER NOT NULL,
                    completed_at INTEGER
                );
                INSERT INTO session (id, directory) VALUES ('sess_1', '/tmp/demo');
                INSERT INTO model_usage (
                    id, session_id, model_id, input_tokens, output_tokens, reasoning_tokens,
                    cache_creation_input_tokens, cache_read_input_tokens, started_at, completed_at
                ) VALUES (
                    'usage-1', 'sess_1', 'GLM-5.2', 100, 20, 0, 0, 64,
                    1700000000000, 1700000001000
                );
                INSERT INTO model_usage (
                    id, session_id, model_id, input_tokens, output_tokens, reasoning_tokens,
                    cache_creation_input_tokens, cache_read_input_tokens, started_at, completed_at
                ) VALUES (
                    'usage-empty', 'sess_1', 'GLM-5-Turbo', 0, 0, 0, 0, 0,
                    1700000002000, 1700000003000
                );
                "#,
            )
            .unwrap();
    }

    fn restore_env(previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(DATA_DIR_ENV, value),
            None => std::env::remove_var(DATA_DIR_ENV),
        }
    }
}
