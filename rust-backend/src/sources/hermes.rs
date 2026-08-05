use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, unix_seconds_to_utc_parts, LocalSession, SourceError};
use rusqlite::{Connection, OpenFlags};

// Requires Cargo.toml dependency:
// rusqlite = { version = "0.31", features = ["bundled"] }
pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let Some(db_path) = home_dir().map(|home| home.join(".hermes").join("state.db")) else {
        return Ok(Vec::new());
    };

    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let connection = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        r#"
        SELECT id, model, started_at, input_tokens, output_tokens, cache_read_tokens,
               cache_write_tokens, estimated_cost_usd, actual_cost_usd
        FROM sessions
        WHERE input_tokens > 0
           OR output_tokens > 0
           OR cache_read_tokens > 0
           OR cache_write_tokens > 0
        "#,
    )?;

    let rows = statement.query_map([], |row| {
        let session_id: Option<String> = row.get(0)?;
        let model_name: Option<String> = row.get(1)?;
        let started_at: Option<f64> = row.get(2)?;
        let input_tokens: Option<i64> = row.get(3)?;
        let output_tokens: Option<i64> = row.get(4)?;
        let cache_read_tokens: Option<i64> = row.get(5)?;
        let cache_write_tokens: Option<i64> = row.get(6)?;
        let estimated_cost: Option<f64> = row.get(7)?;
        let actual_cost: Option<f64> = row.get(8)?;

        let stored_cost = actual_cost
            .filter(|cost| *cost > 0.0)
            .unwrap_or_else(|| estimated_cost.unwrap_or_default());
        let total_cost = if stored_cost > 0.0 {
            stored_cost
        } else {
            model_cost_usd(
                model_name.as_deref().unwrap_or("unknown"),
                TokenUsage {
                    input_tokens: input_tokens.unwrap_or_default(),
                    output_tokens: output_tokens.unwrap_or_default(),
                    cache_creation_tokens: cache_write_tokens.unwrap_or_default(),
                    cache_read_tokens: cache_read_tokens.unwrap_or_default(),
                },
            )
        };

        Ok((
            session_id.unwrap_or_default(),
            model_name.unwrap_or_else(|| "unknown".to_string()),
            started_at.unwrap_or_default(),
            input_tokens.unwrap_or_default(),
            output_tokens.unwrap_or_default(),
            cache_read_tokens.unwrap_or_default(),
            cache_write_tokens.unwrap_or_default(),
            total_cost,
        ))
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        let (
            session_id,
            model_name,
            started_at,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            total_cost,
        ) = row?;

        let Some((date, time)) = unix_seconds_to_utc_parts(started_at) else {
            continue;
        };

        let session = LocalSession {
            session_id,
            date,
            time,
            model_name,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            total_tokens_override: None,
            total_cost,
            model_breakdowns: Vec::new(),
        };

        if session.total_tokens() > 0 {
            sessions.push(session);
        }
    }

    Ok(sessions)
}
