use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, unix_millis_to_utc_parts, LocalSession, SourceError};
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;

const DATA_DIR_ENV: &str = "CLAUDE_SCIENCE_DATA_DIR";
const ALT_DATA_DIR_ENV: &str = "TOKEN_USAGE_CLAUDE_SCIENCE_DATA_DIR";
const DB_FILE: &str = "operon-cli.db";
const UNKNOWN_MODEL: &str = "unknown";

pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let Some(data_dir) = discover_data_dir() else {
        return Ok(Vec::new());
    };

    let db_path = data_dir.join(DB_FILE);
    if !db_path.is_file() {
        return Ok(Vec::new());
    }

    let connection =
        Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        r#"
        SELECT
            id,
            agent_name,
            model,
            effort,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            total_cost,
            created_at,
            project_id
        FROM frames
        WHERE COALESCE(input_tokens, 0) > 0 OR COALESCE(output_tokens, 0) > 0
        ORDER BY created_at
        "#,
    )?;

    let rows = statement.query_map([], |row| {
        Ok(FrameRow {
            id: row.get(0)?,
            agent_name: row.get::<_, Option<String>>(1)?,
            model: row.get::<_, Option<String>>(2)?,
            effort: row.get::<_, Option<String>>(3)?,
            input_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
            output_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or_default(),
            cache_read_tokens: row.get::<_, Option<i64>>(6)?.unwrap_or_default(),
            cache_write_tokens: row.get::<_, Option<i64>>(7)?.unwrap_or_default(),
            total_cost: row.get::<_, Option<f64>>(8)?.unwrap_or_default(),
            created_at: row.get::<_, Option<i64>>(9)?.unwrap_or_default(),
        })
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        let row = row?;
        if let Some(session) = frame_to_session(&row) {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

#[derive(Debug)]
struct FrameRow {
    id: String,
    agent_name: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    total_cost: f64,
    created_at: i64,
}

fn frame_to_session(row: &FrameRow) -> Option<LocalSession> {
    let total_tokens =
        row.input_tokens + row.output_tokens + row.cache_read_tokens + row.cache_write_tokens;
    if total_tokens <= 0 {
        return None;
    }

    let (date, time) = unix_millis_to_utc_parts(row.created_at)?;
    let model_name = format_model_name(row.model.as_deref(), row.effort.as_deref());
    let usage = TokenUsage {
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cache_creation_tokens: row.cache_write_tokens,
        cache_read_tokens: row.cache_read_tokens,
    };
    let total_cost = if row.total_cost > 0.0 {
        row.total_cost
    } else {
        model_cost_usd(&model_name, usage)
    };

    Some(LocalSession {
        session_id: session_id(&row.id, row.agent_name.as_deref()),
        date,
        time,
        model_name,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cache_creation_tokens: row.cache_write_tokens,
        cache_read_tokens: row.cache_read_tokens,
        total_tokens_override: None,
        total_cost,
        model_breakdowns: Vec::new(),
    })
}

fn session_id(frame_id: &str, agent_name: Option<&str>) -> String {
    match agent_name.filter(|name| !name.is_empty()) {
        Some(agent) => format!("{frame_id}:{agent}"),
        None => frame_id.to_string(),
    }
}

fn format_model_name(model: Option<&str>, effort: Option<&str>) -> String {
    let model = model
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(UNKNOWN_MODEL);
    match effort.map(str::trim).filter(|text| !text.is_empty()) {
        Some(effort) => format!("{model} ({effort})"),
        None => model.to_string(),
    }
}

fn discover_data_dir() -> Option<PathBuf> {
    for env_name in [DATA_DIR_ENV, ALT_DATA_DIR_ENV] {
        if let Ok(path) = std::env::var(env_name) {
            let path = PathBuf::from(path);
            if path.is_dir() {
                return Some(path);
            }
        }
    }

    let home = home_dir()?;
    let path = home.join(".claude-science");
    if path.is_dir() {
        return Some(path);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::fs;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn loads_frame_usage_from_operon_db() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        seed_db(&fixture.db_path);

        let previous = std::env::var_os(DATA_DIR_ENV);
        std::env::set_var(DATA_DIR_ENV, &fixture.data_dir);
        let sessions = load_sessions().unwrap();
        restore_env(previous);

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].input_tokens, 100);
        assert_eq!(sessions[0].output_tokens, 40);
        assert_eq!(sessions[0].cache_creation_tokens, 10);
        assert_eq!(sessions[0].cache_read_tokens, 5);
        assert_eq!(sessions[0].total_cost, 0.12);
        assert_eq!(sessions[0].model_name, "claude-sonnet-5 (high)");
        assert_eq!(sessions[1].input_tokens, 20);
        assert_eq!(sessions[1].session_id, "frame-b:REVIEWER");
    }

    #[test]
    fn ignores_demo_frames_without_column_token_counts() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let fixture = TestFixture::new();
        seed_db(&fixture.db_path);
        insert_frame(
            &fixture.db_path,
            "demo-frame",
            "OPERON",
            "claude-opus-4-8",
            None,
            0,
            0,
            0,
            0,
            0.0,
            1_700_000_000_000,
            Some("proj_example"),
        );

        let previous = std::env::var_os(DATA_DIR_ENV);
        std::env::set_var(DATA_DIR_ENV, &fixture.data_dir);
        let sessions = load_sessions().unwrap();
        restore_env(previous);

        assert_eq!(sessions.len(), 2);
        assert!(!sessions.iter().any(|session| session.session_id.starts_with("demo-frame")));
    }

    fn seed_db(db_path: &Path) {
        let connection = Connection::open(db_path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE frames (
                    id TEXT PRIMARY KEY NOT NULL,
                    agent_name TEXT,
                    model TEXT,
                    effort TEXT,
                    input_tokens INTEGER,
                    output_tokens INTEGER,
                    cache_read_tokens INTEGER,
                    cache_write_tokens INTEGER,
                    total_cost REAL,
                    created_at INTEGER,
                    project_id TEXT
                );
                "#,
            )
            .unwrap();

        insert_frame_conn(
            &connection,
            "frame-a",
            "OPERON",
            "claude-sonnet-5",
            Some("high"),
            100,
            40,
            5,
            10,
            0.12,
            1_704_067_200_000,
            None,
        );
        insert_frame_conn(
            &connection,
            "frame-b",
            "REVIEWER",
            "claude-sonnet-5",
            None,
            20,
            10,
            0,
            4,
            0.03,
            1_704_067_300_000,
            None,
        );
        insert_frame_conn(
            &connection,
            "frame-zero",
            "OPERON",
            "claude-sonnet-5",
            None,
            0,
            0,
            0,
            0,
            0.0,
            1_704_067_400_000,
            None,
        );
    }

    fn insert_frame(
        db_path: &Path,
        id: &str,
        agent_name: &str,
        model: &str,
        effort: Option<&str>,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cache_write_tokens: i64,
        total_cost: f64,
        created_at: i64,
        project_id: Option<&str>,
    ) {
        let connection = Connection::open(db_path).unwrap();
        insert_frame_conn(
            &connection,
            id,
            agent_name,
            model,
            effort,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            total_cost,
            created_at,
            project_id,
        );
    }

    fn insert_frame_conn(
        connection: &Connection,
        id: &str,
        agent_name: &str,
        model: &str,
        effort: Option<&str>,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cache_write_tokens: i64,
        total_cost: f64,
        created_at: i64,
        project_id: Option<&str>,
    ) {
        connection
            .execute(
                r#"
                INSERT INTO frames (
                    id, agent_name, model, effort,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                    total_cost, created_at, project_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                rusqlite::params![
                    id,
                    agent_name,
                    model,
                    effort,
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    cache_write_tokens,
                    total_cost,
                    created_at,
                    project_id,
                ],
            )
            .unwrap();
    }

    fn restore_env(previous: Option<std::ffi::OsString>) {
        if let Some(value) = previous {
            std::env::set_var(DATA_DIR_ENV, value);
        } else {
            std::env::remove_var(DATA_DIR_ENV);
        }
    }

    struct TestFixture {
        data_dir: PathBuf,
        db_path: PathBuf,
    }

    impl TestFixture {
        fn new() -> Self {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let data_dir = std::env::temp_dir().join(format!(
                "token-usage-claude-science-{}-{now}",
                std::process::id()
            ));
            fs::create_dir_all(&data_dir).unwrap();
            let db_path = data_dir.join(DB_FILE);
            Self { data_dir, db_path }
        }
    }

    impl Drop for TestFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.data_dir);
        }
    }
}
