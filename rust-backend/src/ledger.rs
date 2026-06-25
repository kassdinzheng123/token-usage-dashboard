use crate::{
    protocol::{Source, View},
    sources,
};
use chrono::{Local, SecondsFormat, Utc};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex as StdMutex},
    thread,
    time::Duration,
};

static INGEST_LOCKS: LazyLock<StdMutex<HashMap<String, Arc<StdMutex<()>>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

const APP_SUPPORT_DIR: &str = "Library/Application Support/Token Usage Dashboard";
const LEDGER_FILE_NAME: &str = "usage-ledger.sqlite";
const LEDGER_PATH_ENV: &str = "TOKEN_USAGE_LEDGER_PATH";

#[derive(Debug, Clone)]
pub struct UsageLedger {
    path: PathBuf,
}

impl UsageLedger {
    pub fn default() -> Result<Self, String> {
        let path = ledger_path()?;
        Self::new(path)
    }

    pub fn new(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        let ledger = Self { path };
        ledger.with_connection(|connection| {
            init_schema(connection)?;
            Ok(())
        })?;
        Ok(ledger)
    }

    pub fn has_source_rows(&self, source: Source) -> Result<bool, String> {
        self.with_connection(|connection| {
            let sessions = connection
                .query_row(
                    "SELECT 1 FROM usage_sessions WHERE source = ?1 LIMIT 1",
                    params![source.to_string()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if sessions {
                return Ok(true);
            }

            Ok(connection
                .query_row(
                    "SELECT 1 FROM usage_blocks WHERE source = ?1 LIMIT 1",
                    params![source.to_string()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }

    pub fn ingest_live_sessions(&self, source: Source, rows: &[Value]) -> Result<(), String> {
        let source_name = source.to_string();
        let lock = ingest_lock(&source_name);
        let _guard = lock
            .lock()
            .map_err(|err| format!("ledger ingest lock poisoned: {err}"))?;
        self.upsert_view_rows(source, View::Sessions, rows)
    }

    pub fn ingest_live_blocks(&self, source: Source, rows: &[Value]) -> Result<(), String> {
        let source_name = source.to_string();
        let lock = ingest_lock(&source_name);
        let _guard = lock
            .lock()
            .map_err(|err| format!("ledger ingest lock poisoned: {err}"))?;
        self.upsert_view_rows(source, View::Blocks, rows)
    }

    pub fn upsert_view_rows(
        &self,
        source: Source,
        view: View,
        rows: &[Value],
    ) -> Result<(), String> {
        match view {
            View::Sessions => self.upsert_session_rows(source, rows),
            View::Blocks => self.upsert_block_rows(source, rows),
            View::Daily | View::Monthly => Ok(()),
        }
    }

    pub fn load_view(&self, source: Source, view: View) -> Result<Vec<Value>, String> {
        match view {
            View::Daily => self.load_daily(source),
            View::Monthly => self.load_monthly(source),
            View::Sessions => self.load_sessions(source),
            View::Blocks => self.load_blocks(source),
        }
    }

    fn upsert_session_rows(&self, source: Source, rows: &[Value]) -> Result<(), String> {
        self.with_connection(|connection| {
            let tx = connection.transaction()?;
            let now = now_iso();
            for row in rows {
                let Some(session_id) = row.get("sessionId").and_then(Value::as_str) else {
                    continue;
                };
                let date = row.get("date").and_then(Value::as_str).unwrap_or_default();
                if date.is_empty() {
                    continue;
                }
                if source == Source::Cursor {
                    delete_cursorpp_legacy_session_ids(&tx, session_id)?;
                }
                tx.execute(
                    r#"
                    INSERT INTO usage_sessions (
                        source, session_id, date, time, input_tokens, output_tokens,
                        cache_creation_tokens, cache_read_tokens, total_tokens, total_cost,
                        models_json, model_breakdowns_json, row_json, first_seen_at, last_seen_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
                    ON CONFLICT(source, session_id) DO UPDATE SET
                        date = excluded.date,
                        time = excluded.time,
                        input_tokens = excluded.input_tokens,
                        output_tokens = excluded.output_tokens,
                        cache_creation_tokens = excluded.cache_creation_tokens,
                        cache_read_tokens = excluded.cache_read_tokens,
                        total_tokens = excluded.total_tokens,
                        total_cost = excluded.total_cost,
                        models_json = excluded.models_json,
                        model_breakdowns_json = excluded.model_breakdowns_json,
                        row_json = excluded.row_json,
                        last_seen_at = excluded.last_seen_at
                    WHERE excluded.total_tokens >= usage_sessions.total_tokens
                       OR (
                            excluded.source = 'cursor'
                            AND usage_sessions.cache_creation_tokens + usage_sessions.cache_read_tokens > 0
                            AND usage_sessions.input_tokens >= excluded.input_tokens
                                + excluded.cache_creation_tokens
                                + excluded.cache_read_tokens
                        )
                    "#,
                    params![
                        source.to_string(),
                        session_id,
                        date,
                        row.get("time").and_then(Value::as_str).unwrap_or_default(),
                        number_i64(row, "inputTokens"),
                        number_i64(row, "outputTokens"),
                        number_i64(row, "cacheCreationTokens"),
                        number_i64(row, "cacheReadTokens"),
                        number_i64(row, "totalTokens"),
                        number_f64(row, "totalCost"),
                        json_array_field(row, "modelsUsed").to_string(),
                        json_array_field(row, "modelBreakdowns").to_string(),
                        row.to_string(),
                        now,
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    fn upsert_block_rows(&self, source: Source, rows: &[Value]) -> Result<(), String> {
        self.with_connection(|connection| {
            let tx = connection.transaction()?;
            let now = now_iso();
            for row in rows {
                let Some(block_id) = row.get("blockId").and_then(Value::as_str) else {
                    continue;
                };
                let date = row.get("date").and_then(Value::as_str).unwrap_or_default();
                if date.is_empty() {
                    continue;
                }
                tx.execute(
                    r#"
                    INSERT INTO usage_blocks (
                        source, block_id, session_id, model_name, timestamp, date, time,
                        input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens,
                        total_tokens, cost, row_json, first_seen_at, last_seen_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
                    ON CONFLICT(source, block_id) DO UPDATE SET
                        session_id = excluded.session_id,
                        model_name = excluded.model_name,
                        timestamp = excluded.timestamp,
                        date = excluded.date,
                        time = excluded.time,
                        input_tokens = excluded.input_tokens,
                        output_tokens = excluded.output_tokens,
                        cache_creation_tokens = excluded.cache_creation_tokens,
                        cache_read_tokens = excluded.cache_read_tokens,
                        total_tokens = excluded.total_tokens,
                        cost = excluded.cost,
                        row_json = excluded.row_json,
                        last_seen_at = excluded.last_seen_at
                    WHERE excluded.total_tokens >= usage_blocks.total_tokens
                    "#,
                    params![
                        source.to_string(),
                        block_id,
                        row.get("sessionId")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                        row.get("modelName")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                        row.get("timestamp")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                        date,
                        row.get("time").and_then(Value::as_str).unwrap_or_default(),
                        number_i64(row, "inputTokens"),
                        number_i64(row, "outputTokens"),
                        number_i64(row, "cacheCreationTokens"),
                        number_i64(row, "cacheReadTokens"),
                        number_i64(row, "totalTokens"),
                        number_f64(row, "cost"),
                        row.to_string(),
                        now,
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    fn load_sessions(&self, source: Source) -> Result<Vec<Value>, String> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                r#"
                SELECT row_json FROM usage_sessions
                WHERE source = ?1
                ORDER BY date, time, session_id
                "#,
            )?;
            let rows =
                statement.query_map(params![source.to_string()], |row| row.get::<_, String>(0))?;

            let mut values = Vec::new();
            for row in rows {
                if let Ok(mut value) = serde_json::from_str::<Value>(&row?) {
                    normalize_clustered_models_in_row(&mut value);
                    values.push(value);
                }
            }
            Ok(values)
        })
    }

    fn load_daily(&self, source: Source) -> Result<Vec<Value>, String> {
        self.load_period(source, Period::Daily)
    }

    fn load_monthly(&self, source: Source) -> Result<Vec<Value>, String> {
        self.load_period(source, Period::Monthly)
    }

    fn load_period(&self, source: Source, period: Period) -> Result<Vec<Value>, String> {
        self.with_connection(|connection| {
            let source_name = source.to_string();
            let select_key = match period {
                Period::Daily => "date",
                Period::Monthly => "substr(date, 1, 7)",
            };
            let sql = format!(
                r#"
                SELECT {select_key} AS period_key,
                       SUM(input_tokens),
                       SUM(output_tokens),
                       SUM(cache_creation_tokens),
                       SUM(cache_read_tokens),
                       SUM(total_tokens),
                       SUM(total_cost)
                FROM usage_sessions
                WHERE source = ?1
                GROUP BY period_key
                ORDER BY period_key
                "#
            );
            let mut statement = connection.prepare(&sql)?;
            let groups = statement.query_map(params![source_name], |row| {
                Ok(PeriodTotals {
                    key: row.get::<_, String>(0)?,
                    input_tokens: row.get::<_, Option<i64>>(1)?.unwrap_or_default(),
                    output_tokens: row.get::<_, Option<i64>>(2)?.unwrap_or_default(),
                    cache_creation_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                    cache_read_tokens: row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
                    total_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or_default(),
                    total_cost: row.get::<_, Option<f64>>(6)?.unwrap_or_default(),
                })
            })?;

            let mut rows = Vec::new();
            for group in groups {
                let group = group?;
                let model_breakdowns =
                    self.period_model_breakdowns(connection, source, period, &group.key)?;
                let models_used = models_used(&model_breakdowns);
                rows.push(match period {
                    Period::Daily => json!({
                        "date": group.key,
                        "inputTokens": group.input_tokens,
                        "outputTokens": group.output_tokens,
                        "cacheCreationTokens": group.cache_creation_tokens,
                        "cacheReadTokens": group.cache_read_tokens,
                        "totalTokens": group.total_tokens,
                        "totalCost": group.total_cost,
                        "modelsUsed": models_used,
                        "modelBreakdowns": model_breakdowns,
                    }),
                    Period::Monthly => json!({
                        "month": group.key,
                        "inputTokens": group.input_tokens,
                        "outputTokens": group.output_tokens,
                        "cacheCreationTokens": group.cache_creation_tokens,
                        "cacheReadTokens": group.cache_read_tokens,
                        "totalTokens": group.total_tokens,
                        "totalCost": group.total_cost,
                        "modelsUsed": models_used,
                        "modelBreakdowns": model_breakdowns,
                    }),
                });
            }
            Ok(rows)
        })
    }

    fn period_model_breakdowns(
        &self,
        connection: &Connection,
        source: Source,
        period: Period,
        key: &str,
    ) -> Result<Vec<Value>, rusqlite::Error> {
        let source_name = source.to_string();
        let (predicate, key_param) = match period {
            Period::Daily => ("date = ?2", key.to_owned()),
            Period::Monthly => ("substr(date, 1, 7) = ?2", key.to_owned()),
        };
        let sql = format!(
            "SELECT model_breakdowns_json FROM usage_sessions WHERE source = ?1 AND {predicate}"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![source_name, key_param], |row| {
            row.get::<_, String>(0)
        })?;

        let mut models = BTreeMap::<String, ModelTotals>::new();
        for row in rows {
            let raw = row?;
            let Ok(value) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let Some(items) = value.as_array() else {
                continue;
            };
            for item in items {
                let model_name = item
                    .get("modelName")
                    .and_then(Value::as_str)
                    .filter(|model| !model.is_empty())
                    .unwrap_or("unknown");
                let clustered = sources::cluster_model_name(model_name);
                models.entry(clustered).or_default().add_breakdown(item);
            }
        }

        Ok(model_breakdowns_from_totals(models))
    }

    fn load_blocks(&self, source: Source) -> Result<Vec<Value>, String> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                r#"
                SELECT row_json FROM usage_blocks
                WHERE source = ?1
                ORDER BY date, time, block_id
                "#,
            )?;
            let rows =
                statement.query_map(params![source.to_string()], |row| row.get::<_, String>(0))?;

            let mut values = Vec::new();
            for row in rows {
                if let Ok(mut value) = serde_json::from_str::<Value>(&row?) {
                    if let Some(model) = value.get("modelName").and_then(Value::as_str) {
                        value["modelName"] = json!(sources::cluster_model_name(model));
                    }
                    values.push(value);
                }
            }
            Ok(values)
        })
    }

    fn with_connection<T>(
        &self,
        mut action: impl FnMut(&mut Connection) -> Result<T, rusqlite::Error>,
    ) -> Result<T, String> {
        let mut last_error = None;
        for attempt in 0..8 {
            let result = self.try_with_connection(&mut action);
            match result {
                Ok(value) => return Ok(value),
                Err(err) if is_sqlite_transient_error(&err) && attempt < 7 => {
                    last_error = Some(err);
                    thread::sleep(Duration::from_millis(25 * (attempt + 1)));
                }
                Err(err) => return Err(format!("sqlite ledger error: {err}")),
            }
        }

        Err(format!(
            "sqlite ledger error: {}",
            last_error
                .map(|err| err.to_string())
                .unwrap_or_else(|| "database remained locked".to_string())
        ))
    }

    fn try_with_connection<T>(
        &self,
        action: &mut impl FnMut(&mut Connection) -> Result<T, rusqlite::Error>,
    ) -> Result<T, rusqlite::Error> {
        let mut connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(30))?;
        action(&mut connection)
    }
}

fn ingest_lock(source: &str) -> Arc<StdMutex<()>> {
    let mut locks = INGEST_LOCKS.lock().expect("ledger ingest lock table poisoned");
    locks
        .entry(source.to_owned())
        .or_insert_with(|| Arc::new(StdMutex::new(())))
        .clone()
}

fn is_sqlite_lock_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(error, _)
            if matches!(error.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

fn is_sqlite_transient_error(err: &rusqlite::Error) -> bool {
    if is_sqlite_lock_error(err) {
        return true;
    }

    matches!(
        err,
        rusqlite::Error::SqliteFailure(error, message)
            if error.code == ErrorCode::ConstraintViolation
                && message.as_deref().is_some_and(|text| {
                    text.contains("usage_sessions.source, usage_sessions.session_id")
                        || text.contains("usage_blocks.source, usage_blocks.block_id")
                })
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Period {
    Daily,
    Monthly,
}

#[derive(Default)]
struct PeriodTotals {
    key: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    total_cost: f64,
}

#[derive(Default)]
struct ModelTotals {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    cost: f64,
}

impl ModelTotals {
    fn add_breakdown(&mut self, row: &Value) {
        self.input_tokens += number_i64(row, "inputTokens");
        self.output_tokens += number_i64(row, "outputTokens");
        self.cache_creation_tokens += number_i64(row, "cacheCreationTokens");
        self.cache_read_tokens += number_i64(row, "cacheReadTokens");
        self.cost += row
            .get("totalCost")
            .map(|_| number_f64(row, "totalCost"))
            .unwrap_or_else(|| number_f64(row, "cost"));
    }
}

fn init_schema(connection: &mut Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS ledger_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS usage_sessions (
            source TEXT NOT NULL,
            session_id TEXT NOT NULL,
            date TEXT NOT NULL,
            time TEXT NOT NULL DEFAULT '',
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            total_cost REAL NOT NULL DEFAULT 0,
            models_json TEXT NOT NULL DEFAULT '[]',
            model_breakdowns_json TEXT NOT NULL DEFAULT '[]',
            row_json TEXT NOT NULL,
            first_seen_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL,
            PRIMARY KEY (source, session_id)
        );

        CREATE INDEX IF NOT EXISTS idx_usage_sessions_source_date
            ON usage_sessions(source, date, time);

        CREATE TABLE IF NOT EXISTS usage_blocks (
            source TEXT NOT NULL,
            block_id TEXT NOT NULL,
            session_id TEXT NOT NULL DEFAULT '',
            model_name TEXT NOT NULL DEFAULT 'unknown',
            timestamp TEXT NOT NULL DEFAULT '',
            date TEXT NOT NULL,
            time TEXT NOT NULL DEFAULT '',
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            cost REAL NOT NULL DEFAULT 0,
            row_json TEXT NOT NULL,
            first_seen_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL,
            PRIMARY KEY (source, block_id)
        );

        CREATE INDEX IF NOT EXISTS idx_usage_blocks_source_date
            ON usage_blocks(source, date, time);
        "#,
    )?;
    migrate_legacy_cursor_source(connection)?;
    connection.execute(
        "INSERT OR REPLACE INTO ledger_meta (key, value) VALUES ('schema_version', '2')",
        [],
    )?;
    Ok(())
}

fn migrate_legacy_cursor_source(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        DELETE FROM usage_sessions
        WHERE source = 'cursorpp'
          AND session_id IN (
              SELECT session_id FROM usage_sessions WHERE source = 'cursor'
          )
        "#,
        [],
    )?;
    connection.execute(
        r#"
        DELETE FROM usage_sessions AS legacy
        WHERE legacy.source = 'cursorpp'
          AND EXISTS (
              SELECT 1
              FROM usage_sessions AS current
              WHERE current.source = 'cursor'
                AND current.session_id = legacy.session_id
                AND current.total_tokens >= legacy.total_tokens
          )
        "#,
        [],
    )?;
    connection.execute(
        r#"
        DELETE FROM usage_sessions AS current
        WHERE current.source = 'cursor'
          AND EXISTS (
              SELECT 1
              FROM usage_sessions AS legacy
              WHERE legacy.source = 'cursorpp'
                AND legacy.session_id = current.session_id
                AND legacy.total_tokens > current.total_tokens
          )
        "#,
        [],
    )?;
    connection.execute(
        "UPDATE usage_sessions SET source = 'cursor' WHERE source = 'cursorpp'",
        [],
    )?;
    connection.execute(
        r#"
        DELETE FROM usage_blocks
        WHERE source = 'cursorpp'
          AND block_id IN (
              SELECT block_id FROM usage_blocks WHERE source = 'cursor'
          )
        "#,
        [],
    )?;
    connection.execute(
        "UPDATE usage_blocks SET source = 'cursor' WHERE source = 'cursorpp'",
        [],
    )?;
    Ok(())
}

fn normalize_clustered_models_in_row(row: &mut Value) {
    if let Some(models) = row.get_mut("modelsUsed").and_then(Value::as_array_mut) {
        for item in models.iter_mut() {
            if let Some(name) = item.as_str() {
                *item = json!(sources::cluster_model_name(name));
            }
        }
    }
    if let Some(breakdowns) = row
        .get_mut("modelBreakdowns")
        .and_then(Value::as_array_mut)
    {
        for item in breakdowns.iter_mut() {
            if let Some(obj) = item.as_object_mut() {
                if let Some(name) = obj.get("modelName").and_then(Value::as_str) {
                    obj.insert("modelName".to_string(), json!(sources::cluster_model_name(name)));
                }
            }
        }
    }
}

fn model_breakdowns_from_totals(models: BTreeMap<String, ModelTotals>) -> Vec<Value> {
    models
        .into_iter()
        .map(|(model_name, totals)| {
            json!({
                "modelName": model_name,
                "inputTokens": totals.input_tokens,
                "outputTokens": totals.output_tokens,
                "cacheCreationTokens": totals.cache_creation_tokens,
                "cacheReadTokens": totals.cache_read_tokens,
                "cost": totals.cost,
            })
        })
        .collect()
}

fn models_used(model_breakdowns: &[Value]) -> Vec<String> {
    model_breakdowns
        .iter()
        .filter_map(|row| row.get("modelName").and_then(Value::as_str))
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn number_i64(row: &Value, field: &str) -> i64 {
    row.get(field).map(sources::to_i64).unwrap_or_default()
}

fn number_f64(row: &Value, field: &str) -> f64 {
    row.get(field).map(sources::num).unwrap_or_default()
}

fn json_array_field(row: &Value, field: &str) -> Value {
    row.get(field)
        .and_then(Value::as_array)
        .map(|items| Value::Array(items.clone()))
        .unwrap_or_else(|| Value::Array(Vec::new()))
}

fn delete_cursorpp_legacy_session_ids(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
) -> Result<(), rusqlite::Error> {
    if !is_stable_cursorpp_fallback_session_id(session_id) {
        return Ok(());
    }
    tx.execute(
        "DELETE FROM usage_sessions WHERE source = ?1 AND session_id GLOB ?2",
        params![
            Source::Cursor.to_string(),
            format!("{session_id}:*:*:*:*")
        ],
    )?;
    Ok(())
}

fn is_stable_cursorpp_fallback_session_id(session_id: &str) -> bool {
    let mut parts = session_id.split(':');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some("cursorpp"), Some(conversation_id), Some(timestamp), None)
            if conversation_id.len() == 36
                && timestamp.len() == 14
                && timestamp.chars().all(|character| character.is_ascii_digit())
    )
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn ledger_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(LEDGER_PATH_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Err("HOME is not set; cannot locate token usage ledger".to_string());
    };
    Ok(home.join(APP_SUPPORT_DIR).join(LEDGER_FILE_NAME))
}

pub fn current_date() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn upsert_preserves_rows_after_source_disappears() {
        let ledger = test_ledger();
        let source = Source::Codex;
        ledger
            .upsert_view_rows(
                source,
                View::Sessions,
                &[json!({
                    "sessionId": "s1",
                    "date": "2026-06-01",
                    "time": "12:00",
                    "inputTokens": 10,
                    "outputTokens": 5,
                    "cacheCreationTokens": 0,
                    "cacheReadTokens": 2,
                    "totalTokens": 17,
                    "totalCost": 0.1,
                    "modelsUsed": ["gpt-5.5"],
                    "modelBreakdowns": [{
                        "modelName": "gpt-5.5",
                        "inputTokens": 10,
                        "outputTokens": 5,
                        "cacheCreationTokens": 0,
                        "cacheReadTokens": 2,
                        "cost": 0.1
                    }]
                })],
            )
            .unwrap();

        assert!(ledger.has_source_rows(source).unwrap());
        let sessions = ledger.load_view(source, View::Sessions).unwrap();
        assert_eq!(sessions.len(), 1);
        let daily = ledger.load_view(source, View::Daily).unwrap();
        assert_eq!(daily[0]["totalTokens"], 17);
        assert_eq!(daily[0]["modelBreakdowns"][0]["modelName"], "gpt-5.5");
    }

    #[test]
    fn upsert_does_not_replace_session_with_lower_token_count() {
        let ledger = test_ledger();
        let source = Source::Codex;
        let full = json!({
            "sessionId": "s1",
            "date": "2026-06-01",
            "time": "12:00",
            "inputTokens": 80,
            "outputTokens": 30,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 20,
            "totalTokens": 130,
            "totalCost": 0.1,
            "modelsUsed": ["gpt-5.5"],
            "modelBreakdowns": [{
                "modelName": "gpt-5.5",
                "inputTokens": 80,
                "outputTokens": 30,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 20,
                "cost": 0.1
            }]
        });
        let cleaned = json!({
            "sessionId": "s1",
            "date": "2026-06-01",
            "time": "12:00",
            "inputTokens": 10,
            "outputTokens": 5,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 0,
            "totalTokens": 15,
            "totalCost": 0.01,
            "modelsUsed": ["gpt-5.5"],
            "modelBreakdowns": [{
                "modelName": "gpt-5.5",
                "inputTokens": 10,
                "outputTokens": 5,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 0,
                "cost": 0.01
            }]
        });

        ledger
            .upsert_view_rows(source, View::Sessions, &[full])
            .unwrap();
        ledger
            .upsert_view_rows(source, View::Sessions, &[cleaned])
            .unwrap();

        let sessions = ledger.load_view(source, View::Sessions).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["totalTokens"], 130);
        assert_eq!(sessions[0]["inputTokens"], 80);
    }

    #[test]
    fn cursorpp_upsert_replaces_legacy_double_counted_cache_rows() {
        let ledger = test_ledger();
        let source = Source::Cursor;
        let legacy = json!({
            "sessionId": "cursorpp:r1",
            "date": "2026-06-01",
            "time": "12:00",
            "inputTokens": 120,
            "outputTokens": 30,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 20,
            "totalTokens": 170,
            "totalCost": 0.1,
            "modelsUsed": ["gpt-5.5"],
            "modelBreakdowns": [{
                "modelName": "gpt-5.5",
                "inputTokens": 120,
                "outputTokens": 30,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 20,
                "cost": 0.1
            }]
        });
        let normalized = json!({
            "sessionId": "cursorpp:r1",
            "date": "2026-06-01",
            "time": "12:00",
            "inputTokens": 100,
            "outputTokens": 30,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 20,
            "totalTokens": 150,
            "totalCost": 0.1,
            "modelsUsed": ["gpt-5.5"],
            "modelBreakdowns": [{
                "modelName": "gpt-5.5",
                "inputTokens": 100,
                "outputTokens": 30,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 20,
                "cost": 0.1
            }]
        });

        ledger
            .upsert_view_rows(source, View::Sessions, &[legacy])
            .unwrap();
        ledger
            .upsert_view_rows(source, View::Sessions, &[normalized])
            .unwrap();

        let sessions = ledger.load_view(source, View::Sessions).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["inputTokens"], 100);
        assert_eq!(sessions[0]["cacheReadTokens"], 20);
        assert_eq!(sessions[0]["totalTokens"], 150);
    }

    #[test]
    fn cursorpp_upsert_removes_legacy_fallback_id_with_token_suffix() {
        let ledger = test_ledger();
        let source = Source::Cursor;
        let stable_session_id = "cursorpp:79bc14d9-e224-4864-a190-d1d0dce210e7:20260606003510";
        let legacy = json!({
            "sessionId": format!("{stable_session_id}:102741:1154:0:74496"),
            "date": "2026-06-06",
            "time": "00:35",
            "inputTokens": 102741,
            "outputTokens": 1154,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 74496,
            "totalTokens": 178391,
            "totalCost": 0.1,
            "modelsUsed": ["gpt-5.5"],
            "modelBreakdowns": [{
                "modelName": "gpt-5.5",
                "inputTokens": 102741,
                "outputTokens": 1154,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 74496,
                "cost": 0.1
            }]
        });
        let normalized = json!({
            "sessionId": stable_session_id,
            "date": "2026-06-06",
            "time": "00:35",
            "inputTokens": 28245,
            "outputTokens": 1154,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 74496,
            "totalTokens": 103895,
            "totalCost": 0.1,
            "modelsUsed": ["gpt-5.5"],
            "modelBreakdowns": [{
                "modelName": "gpt-5.5",
                "inputTokens": 28245,
                "outputTokens": 1154,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 74496,
                "cost": 0.1
            }]
        });

        ledger
            .upsert_view_rows(source, View::Sessions, &[legacy])
            .unwrap();
        ledger
            .upsert_view_rows(source, View::Sessions, &[normalized])
            .unwrap();

        let sessions = ledger.load_view(source, View::Sessions).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["sessionId"], stable_session_id);
        assert_eq!(sessions[0]["inputTokens"], 28245);
        assert_eq!(sessions[0]["totalTokens"], 103895);
    }

    fn test_ledger() -> UsageLedger {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("token-usage-ledger-test-{stamp}"))
            .join("usage-ledger.sqlite");
        UsageLedger::new(path).unwrap()
    }
}
