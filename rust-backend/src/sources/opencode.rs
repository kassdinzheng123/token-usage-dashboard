use crate::pricing::{model_cost_usd, TokenUsage};
use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryFrom;
use std::fs;
use std::path::{Path, PathBuf};

const OPENCODE_DATA_DIR_ENV: &str = "OPENCODE_DATA_DIR";

#[derive(Debug, Clone)]
struct UsageEntry {
    message_id: String,
    session_id: String,
    timestamp_ms: i64,
    model_name: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_cost: f64,
}

impl UsageEntry {
    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Clone)]
struct SessionMetadata {
    id: String,
    title: String,
}

#[derive(Default)]
struct AggregateUsage {
    key: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    total_cost: f64,
    models_used: Vec<String>,
    model_breakdowns: BTreeMap<String, ModelBreakdown>,
}

#[derive(Default)]
struct ModelBreakdown {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    cost: f64,
}

pub fn load_source_view(view: &str, refresh: bool) -> Result<Vec<Value>, String> {
    load_source_view_since(view, refresh, None)
}

pub fn load_source_view_since(
    view: &str,
    _refresh: bool,
    watermark_ms: Option<i64>,
) -> Result<Vec<Value>, String> {
    if view == "blocks" {
        return Ok(Vec::new());
    }

    if view == "daily" {
        if let Some(rows) = load_daily_from_sqlite(None, None) {
            return Ok(rows);
        }
    }

    let entries = load_usage_entries(watermark_ms);
    let sessions = load_session_metadata();

    match view {
        "daily" => Ok(entries_to_daily(&entries)),
        "monthly" => Ok(entries_to_monthly(&entries)),
        "sessions" => Ok(entries_to_sessions(&entries, &sessions)),
        "messages" => Ok(entries_to_messages(&entries)),
        other => Err(format!("unsupported view: {other}")),
    }
}

pub fn load_daily_for_date(date: &str, refresh: bool) -> Result<Vec<Value>, String> {
    let _ = refresh;
    let Some((start_ms, end_ms)) = local_day_bounds_ms(date) else {
        return Ok(Vec::new());
    };

    if let Some(rows) = load_daily_from_sqlite(Some(start_ms), Some(end_ms)) {
        return Ok(rows
            .into_iter()
            .filter(|row| row.get("date").and_then(Value::as_str) == Some(date))
            .collect());
    }

    let entries = load_usage_entries_between(start_ms, end_ms);
    Ok(entries_to_daily(&entries)
        .into_iter()
        .filter(|row| row.get("date").and_then(Value::as_str) == Some(date))
        .collect())
}

fn load_usage_entries(watermark_ms: Option<i64>) -> Vec<UsageEntry> {
    let storage = storage_dir();
    let mut seen = HashSet::new();
    let mut entries = Vec::new();

    if let Some(storage) = storage.as_deref() {
        if !append_sqlite_messages(&mut entries, &mut seen, storage) {
            append_json_messages(&mut entries, &mut seen, &storage.join("message"), watermark_ms);
        }
    }

    entries
}

fn load_usage_entries_between(start_ms: i64, end_ms: i64) -> Vec<UsageEntry> {
    let storage = storage_dir();
    let mut seen = HashSet::new();
    let mut entries = Vec::new();

    if let Some(storage) = storage.as_deref() {
        if !append_sqlite_messages_between(&mut entries, &mut seen, storage, start_ms, end_ms) {
            append_json_messages(&mut entries, &mut seen, &storage.join("message"), None);
            entries.retain(|entry| entry.timestamp_ms >= start_ms && entry.timestamp_ms < end_ms);
        }
    }

    entries
}

fn load_daily_from_sqlite(start_ms: Option<i64>, end_ms: Option<i64>) -> Option<Vec<Value>> {
    let storage = storage_dir()?;
    let mut entries = Vec::new();
    if append_sqlite_daily_entries(&mut entries, &storage, start_ms, end_ms) {
        Some(entries_to_daily(&entries))
    } else {
        None
    }
}

fn load_session_metadata() -> HashMap<String, SessionMetadata> {
    let mut sessions = HashMap::new();
    let Some(storage) = storage_dir() else {
        return sessions;
    };

    append_json_sessions(&mut sessions, &storage.join("session"));
    append_sqlite_sessions(&mut sessions, &storage);
    sessions
}

fn append_json_messages(
    entries: &mut Vec<UsageEntry>,
    seen: &mut HashSet<String>,
    messages_dir: &Path,
    watermark_ms: Option<i64>,
) {
    let mut files = Vec::new();
    collect_json_files(messages_dir, &mut files);
    files.sort();

    for file in files {
        if let Some(watermark) = watermark_ms {
            if !super::file_modified_after(&file, watermark) {
                continue;
            }
        }
        let Ok(contents) = fs::read_to_string(file) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        append_message_value(entries, seen, &value, None, None, None);
    }
}

fn append_json_sessions(sessions: &mut HashMap<String, SessionMetadata>, sessions_dir: &Path) {
    let mut files = Vec::new();
    collect_json_files(sessions_dir, &mut files);
    files.sort();

    for file in files {
        let Ok(contents) = fs::read_to_string(file) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        if let Some(session) = session_from_value(&value) {
            sessions.insert(session.id.clone(), session);
        }
    }
}

fn append_sqlite_messages(
    entries: &mut Vec<UsageEntry>,
    seen: &mut HashSet<String>,
    storage: &Path,
) -> bool {
    let Some(db_path) = opencode_db_path(storage) else {
        return false;
    };
    let Ok(connection) = open_opencode_readonly(&db_path) else {
        return false;
    };
    let Ok(mut statement) = connection.prepare(
        r#"
        SELECT id, session_id, time_created, data
        FROM message
        WHERE data IS NOT NULL
        ORDER BY time_created
        "#,
    ) else {
        return false;
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    }) else {
        return false;
    };

    for row in rows.flatten() {
        let (id, session_id, time_created, data) = row;
        let Some(data) = data else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if string_field(value.get("role")).as_deref() != Some("assistant") {
            continue;
        }
        append_message_value(
            entries,
            seen,
            &value,
            id.as_deref(),
            session_id.as_deref(),
            time_created,
        );
    }

    true
}

fn append_sqlite_messages_between(
    entries: &mut Vec<UsageEntry>,
    seen: &mut HashSet<String>,
    storage: &Path,
    start_ms: i64,
    end_ms: i64,
) -> bool {
    let Some(db_path) = opencode_db_path(storage) else {
        return false;
    };
    let Ok(connection) = open_opencode_readonly(&db_path) else {
        return false;
    };
    let Ok(mut statement) = connection.prepare(
        r#"
        SELECT id, session_id, time_created, data
        FROM message
        WHERE data IS NOT NULL
          AND time_created >= ?1
          AND time_created < ?2
        ORDER BY time_created
        "#,
    ) else {
        return false;
    };
    let Ok(rows) = statement.query_map([start_ms, end_ms], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    }) else {
        return false;
    };

    for row in rows.flatten() {
        let (id, session_id, time_created, data) = row;
        let Some(data) = data else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if string_field(value.get("role")).as_deref() != Some("assistant") {
            continue;
        }
        append_message_value(
            entries,
            seen,
            &value,
            id.as_deref(),
            session_id.as_deref(),
            time_created,
        );
    }

    true
}

fn append_sqlite_daily_entries(
    entries: &mut Vec<UsageEntry>,
    storage: &Path,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
) -> bool {
    let Some(db_path) = opencode_db_path(storage) else {
        return false;
    };
    try_prepare_opencode_indexes(&db_path);
    let Ok(connection) = open_opencode_readonly(&db_path) else {
        return false;
    };

    let select = r#"
        SELECT
          date(COALESCE(json_extract(data, '$.time.created'), time_created) / 1000, 'unixepoch', 'localtime') AS usage_date,
          COALESCE(json_extract(data, '$.modelID'), json_extract(data, '$.model'), 'unknown') AS model_name,
          COALESCE(json_extract(data, '$.providerID'), json_extract(data, '$.provider'), '') AS provider_name,
          SUM(COALESCE(
            json_extract(data, '$.tokens.input'),
            json_extract(data, '$.tokens.inputTokens'),
            json_extract(data, '$.tokens.input_tokens'),
            json_extract(data, '$.usage.input'),
            json_extract(data, '$.usage.inputTokens'),
            json_extract(data, '$.usage.input_tokens'),
            0
          )) AS input_tokens,
          SUM(COALESCE(
            json_extract(data, '$.tokens.output'),
            json_extract(data, '$.tokens.outputTokens'),
            json_extract(data, '$.tokens.output_tokens'),
            json_extract(data, '$.usage.output'),
            json_extract(data, '$.usage.outputTokens'),
            json_extract(data, '$.usage.output_tokens'),
            0
          )) AS output_tokens,
          SUM(COALESCE(
            json_extract(data, '$.tokens.cache.write'),
            json_extract(data, '$.tokens.cacheCreation'),
            json_extract(data, '$.tokens.cacheCreationTokens'),
            json_extract(data, '$.tokens.cacheCreationInputTokens'),
            json_extract(data, '$.tokens.cache_creation_input_tokens'),
            json_extract(data, '$.usage.cache.write'),
            json_extract(data, '$.usage.cacheCreation'),
            json_extract(data, '$.usage.cacheCreationTokens'),
            json_extract(data, '$.usage.cacheCreationInputTokens'),
            json_extract(data, '$.usage.cache_creation_input_tokens'),
            0
          )) AS cache_creation_tokens,
          SUM(COALESCE(
            json_extract(data, '$.tokens.cache.read'),
            json_extract(data, '$.tokens.cacheRead'),
            json_extract(data, '$.tokens.cacheReadTokens'),
            json_extract(data, '$.tokens.cacheReadInputTokens'),
            json_extract(data, '$.tokens.cache_read_input_tokens'),
            json_extract(data, '$.usage.cache.read'),
            json_extract(data, '$.usage.cacheRead'),
            json_extract(data, '$.usage.cacheReadTokens'),
            json_extract(data, '$.usage.cacheReadInputTokens'),
            json_extract(data, '$.usage.cache_read_input_tokens'),
            0
          )) AS cache_read_tokens,
          SUM(COALESCE(json_extract(data, '$.costUSD'), json_extract(data, '$.cost'), 0)) AS total_cost
        FROM message
        WHERE data IS NOT NULL
          AND json_extract(data, '$.role') = 'assistant'
    "#;
    let group = r#"
        GROUP BY usage_date, provider_name, model_name
        HAVING input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens > 0
        ORDER BY usage_date
    "#;

    let query_with_range = format!("{select} AND time_created >= ?1 AND time_created < ?2 {group}");
    let query_without_range = format!("{select} {group}");

    if let (Some(start_ms), Some(end_ms)) = (start_ms, end_ms) {
        let Ok(mut statement) = connection.prepare(&query_with_range) else {
            return false;
        };
        let Ok(rows) = statement.query_map(params![start_ms, end_ms], sqlite_daily_entry) else {
            return false;
        };
        for entry in rows.flatten() {
            entries.push(entry);
        }
    } else {
        let Ok(mut statement) = connection.prepare(&query_without_range) else {
            return false;
        };
        let Ok(rows) = statement.query_map([], sqlite_daily_entry) else {
            return false;
        };
        for entry in rows.flatten() {
            entries.push(entry);
        }
    }

    true
}

fn sqlite_daily_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageEntry> {
    let date: String = row.get::<_, Option<String>>(0)?.unwrap_or_default();
    let model_name = row
        .get::<_, Option<String>>(1)?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let provider = row.get::<_, Option<String>>(2)?.unwrap_or_default();
    let input_tokens = sql_number_to_i64(row.get::<_, Option<f64>>(3)?);
    let output_tokens = sql_number_to_i64(row.get::<_, Option<f64>>(4)?);
    let cache_creation_tokens = sql_number_to_i64(row.get::<_, Option<f64>>(5)?);
    let cache_read_tokens = sql_number_to_i64(row.get::<_, Option<f64>>(6)?);
    let explicit_cost = row.get::<_, Option<f64>>(7)?.unwrap_or_default();

    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    };
    let provider_model = format!("{provider}/{model_name}");
    let total_cost = if explicit_cost > 0.0 {
        explicit_cost
    } else {
        let provider_cost = model_cost_usd(&provider_model, usage);
        if provider_cost > 0.0 {
            provider_cost
        } else {
            model_cost_usd(&model_name, usage)
        }
    };

    Ok(UsageEntry {
        message_id: format!("sqlite:{date}:{provider}:{model_name}"),
        session_id: "daily".to_string(),
        timestamp_ms: local_day_bounds_ms(&date)
            .map(|(start_ms, _)| start_ms)
            .unwrap_or_else(current_timestamp_ms),
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_cost,
    })
}

fn sql_number_to_i64(value: Option<f64>) -> i64 {
    value
        .filter(|number| number.is_finite())
        .map(|number| number.round() as i64)
        .unwrap_or_default()
}

fn append_sqlite_sessions(sessions: &mut HashMap<String, SessionMetadata>, storage: &Path) {
    let Some(db_path) = opencode_db_path(storage) else {
        return;
    };
    let Ok(connection) = open_opencode_readonly(&db_path) else {
        return;
    };
    let Ok(mut statement) = connection.prepare("SELECT id, title FROM session") else {
        return;
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
        ))
    }) else {
        return;
    };

    for row in rows.flatten() {
        let (id, title) = row;
        let Some(id) = id.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        sessions
            .entry(id.clone())
            .or_insert_with(|| SessionMetadata {
                title: title
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| id.clone()),
                id,
            });
    }
}

fn append_message_value(
    entries: &mut Vec<UsageEntry>,
    seen: &mut HashSet<String>,
    value: &Value,
    fallback_id: Option<&str>,
    fallback_session_id: Option<&str>,
    fallback_timestamp_ms: Option<i64>,
) {
    let Some(entry) = usage_entry_from_value(
        value,
        fallback_id,
        fallback_session_id,
        fallback_timestamp_ms,
    ) else {
        return;
    };
    if seen.insert(entry.message_id.clone()) {
        entries.push(entry);
    }
}

fn usage_entry_from_value(
    value: &Value,
    fallback_id: Option<&str>,
    fallback_session_id: Option<&str>,
    fallback_timestamp_ms: Option<i64>,
) -> Option<UsageEntry> {
    let object = value.as_object()?;
    let message_id = string_field(object.get("id"))
        .or_else(|| fallback_id.map(ToOwned::to_owned))
        .filter(|id| !id.trim().is_empty())?;
    let provider =
        string_field(object.get("providerID")).or_else(|| string_field(object.get("provider")));
    let model_name =
        string_field(object.get("modelID")).or_else(|| string_field(object.get("model")));

    if provider.as_deref().unwrap_or_default().is_empty()
        || model_name.as_deref().unwrap_or_default().is_empty()
    {
        return None;
    }

    let (input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens) =
        token_counts(object)?;
    if input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens <= 0 {
        return None;
    }

    let time = object.get("time").and_then(Value::as_object);
    let timestamp_ms = time
        .and_then(|time| time.get("created").map(to_i64))
        .or_else(|| object.get("timestamp").map(to_i64))
        .or(fallback_timestamp_ms)
        .map(normalize_timestamp_ms)
        .unwrap_or_else(current_timestamp_ms);

    let provider = provider.unwrap_or_default();
    let model_name = model_name.unwrap_or_default();
    let explicit_cost = object
        .get("costUSD")
        .map(num)
        .or_else(|| object.get("cost").map(num))
        .unwrap_or_default();
    let total_cost = if explicit_cost > 0.0 {
        explicit_cost
    } else {
        let usage = TokenUsage {
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        };
        let provider_model = format!("{provider}/{model_name}");
        let provider_cost = model_cost_usd(&provider_model, usage);
        if provider_cost > 0.0 {
            provider_cost
        } else {
            model_cost_usd(&model_name, usage)
        }
    };

    Some(UsageEntry {
        message_id,
        session_id: string_field(object.get("sessionID"))
            .or_else(|| string_field(object.get("sessionId")))
            .or_else(|| fallback_session_id.map(ToOwned::to_owned))
            .unwrap_or_else(|| "unknown".to_string()),
        timestamp_ms,
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_cost,
    })
}

fn token_counts(object: &Map<String, Value>) -> Option<(i64, i64, i64, i64)> {
    let tokens = object.get("tokens").and_then(Value::as_object);
    let usage = object.get("usage").and_then(Value::as_object);
    let token_source = tokens.or(usage)?;

    let input = first_i64(token_source, &["input", "inputTokens", "input_tokens"]);
    let output = first_i64(token_source, &["output", "outputTokens", "output_tokens"]);
    let cache_creation = first_i64(
        token_source,
        &[
            "cacheCreation",
            "cacheCreationTokens",
            "cacheCreationInputTokens",
            "cache_creation_input_tokens",
        ],
    )
    .or_else(|| {
        token_source
            .get("cache")
            .and_then(Value::as_object)
            .and_then(|cache| first_i64(cache, &["write", "creation"]))
    });
    let cache_read = first_i64(
        token_source,
        &[
            "cacheRead",
            "cacheReadTokens",
            "cacheReadInputTokens",
            "cache_read_input_tokens",
        ],
    )
    .or_else(|| {
        token_source
            .get("cache")
            .and_then(Value::as_object)
            .and_then(|cache| first_i64(cache, &["read"]))
    });

    Some((
        input.unwrap_or_default(),
        output.unwrap_or_default(),
        cache_creation.unwrap_or_default(),
        cache_read.unwrap_or_default(),
    ))
}

fn session_from_value(value: &Value) -> Option<SessionMetadata> {
    let id = string_field(value.get("id"))?;
    if id.trim().is_empty() {
        return None;
    }
    Some(SessionMetadata {
        title: string_field(value.get("title")).unwrap_or_else(|| id.clone()),
        id,
    })
}

fn entries_to_daily(entries: &[UsageEntry]) -> Vec<Value> {
    aggregate_entries(entries, |entry| local_parts(entry.timestamp_ms).0)
        .into_iter()
        .map(|group| {
            json!({
                "date": group.key,
                "inputTokens": group.input_tokens,
                "outputTokens": group.output_tokens,
                "cacheCreationTokens": group.cache_creation_tokens,
                "cacheReadTokens": group.cache_read_tokens,
                "totalTokens": group.total_tokens,
                "totalCost": group.total_cost,
                "modelsUsed": group.models_used,
                "modelBreakdowns": model_breakdowns_to_json(group.model_breakdowns),
            })
        })
        .collect()
}

fn entries_to_monthly(entries: &[UsageEntry]) -> Vec<Value> {
    aggregate_entries(entries, |entry| {
        local_parts(entry.timestamp_ms).0[..7].to_string()
    })
    .into_iter()
    .map(|group| {
        json!({
            "month": group.key,
            "inputTokens": group.input_tokens,
            "outputTokens": group.output_tokens,
            "cacheCreationTokens": group.cache_creation_tokens,
            "cacheReadTokens": group.cache_read_tokens,
            "totalTokens": group.total_tokens,
            "totalCost": group.total_cost,
            "modelsUsed": group.models_used,
            "modelBreakdowns": model_breakdowns_to_json(group.model_breakdowns),
        })
    })
    .collect()
}

/// One row per assistant message, keyed by the message id already used for
/// cross-file dedupe, so ledger upserts stay idempotent.
fn entries_to_messages(entries: &[UsageEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| {
            let (date, time) = local_parts(entry.timestamp_ms);
            json!({
                "messageId": entry.message_id,
                "sessionId": entry.session_id,
                "date": date,
                "time": time,
                "inputTokens": entry.input_tokens,
                "outputTokens": entry.output_tokens,
                "cacheCreationTokens": entry.cache_creation_tokens,
                "cacheReadTokens": entry.cache_read_tokens,
                "totalTokens": entry.total_tokens(),
                "cost": entry.total_cost,
            })
        })
        .collect()
}

fn entries_to_sessions(
    entries: &[UsageEntry],
    sessions: &HashMap<String, SessionMetadata>,
) -> Vec<Value> {
    let mut last_activity_by_session: HashMap<String, i64> = HashMap::new();
    for entry in entries {
        let last_activity = last_activity_by_session
            .entry(entry.session_id.clone())
            .or_insert(entry.timestamp_ms);
        if entry.timestamp_ms > *last_activity {
            *last_activity = entry.timestamp_ms;
        }
    }

    let mut rows: Vec<_> = aggregate_entries(entries, |entry| entry.session_id.clone())
        .into_iter()
        .map(|group| {
            let timestamp_ms = last_activity_by_session
                .get(&group.key)
                .copied()
                .unwrap_or_default();
            let (date, time) = local_parts(timestamp_ms);
            let title = sessions
                .get(&group.key)
                .map(|session| session.title.clone())
                .unwrap_or_else(|| group.key.clone());
            json!({
                "sessionId": group.key,
                "sessionTitle": title,
                "date": date,
                "time": time,
                "inputTokens": group.input_tokens,
                "outputTokens": group.output_tokens,
                "cacheCreationTokens": group.cache_creation_tokens,
                "cacheReadTokens": group.cache_read_tokens,
                "totalTokens": group.total_tokens,
                "totalCost": group.total_cost,
                "modelsUsed": group.models_used,
                "modelBreakdowns": model_breakdowns_to_json(group.model_breakdowns),
            })
        })
        .collect();
    rows.sort_by_key(sort_key);
    rows
}

fn aggregate_entries(
    entries: &[UsageEntry],
    key_for: impl Fn(&UsageEntry) -> String,
) -> Vec<AggregateUsage> {
    let mut groups: BTreeMap<String, AggregateUsage> = BTreeMap::new();

    for entry in entries {
        let key = key_for(entry);
        let group = groups.entry(key.clone()).or_insert_with(|| AggregateUsage {
            key: key.clone(),
            ..AggregateUsage::default()
        });

        group.input_tokens += entry.input_tokens;
        group.output_tokens += entry.output_tokens;
        group.cache_creation_tokens += entry.cache_creation_tokens;
        group.cache_read_tokens += entry.cache_read_tokens;
        group.total_tokens += entry.total_tokens();
        group.total_cost += entry.total_cost;

        let model_name = super::cluster_model_name_at(&entry.model_name, Some(&key));
        if !group.models_used.contains(&model_name) {
            group.models_used.push(model_name.clone());
        }

        let model = group.model_breakdowns.entry(model_name).or_default();
        model.input_tokens += entry.input_tokens;
        model.output_tokens += entry.output_tokens;
        model.cache_creation_tokens += entry.cache_creation_tokens;
        model.cache_read_tokens += entry.cache_read_tokens;
        model.cost += entry.total_cost;
    }

    groups.into_values().collect()
}

fn model_breakdowns_to_json(model_breakdowns: BTreeMap<String, ModelBreakdown>) -> Vec<Value> {
    model_breakdowns
        .into_iter()
        .map(|(model_name, breakdown)| {
            json!({
                "modelName": model_name,
                "inputTokens": breakdown.input_tokens,
                "outputTokens": breakdown.output_tokens,
                "cacheCreationTokens": breakdown.cache_creation_tokens,
                "cacheReadTokens": breakdown.cache_read_tokens,
                "cost": breakdown.cost,
            })
        })
        .collect()
}

fn collect_json_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
}

pub(crate) fn storage_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(OPENCODE_DATA_DIR_ENV) {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            let path = absolute_path(path);
            return Some(
                if path.file_name().and_then(|name| name.to_str()) == Some("storage") {
                    path
                } else {
                    path.join("storage")
                },
            );
        }
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local/share/opencode/storage"))
}

pub(crate) fn opencode_db_path(storage: &Path) -> Option<PathBuf> {
    storage
        .parent()
        .map(|base| base.join("opencode.db"))
        .filter(|path| path.exists())
}

pub(crate) fn open_opencode_readonly(db_path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

fn try_prepare_opencode_indexes(db_path: &Path) {
    let Ok(connection) = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return;
    };

    let _ = connection.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_token_usage_message_time_created
        ON message(time_created)
        WHERE data IS NOT NULL;
        "#,
    );
}

fn absolute_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(&path))
        .unwrap_or(path)
}

fn local_parts(timestamp_ms: i64) -> (String, String) {
    let fallback = Local.timestamp_millis_opt(0).single().unwrap();
    let date_time = Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .unwrap_or(fallback);
    (
        date_time.format("%Y-%m-%d").to_string(),
        date_time.format("%H:%M").to_string(),
    )
}

fn local_day_bounds_ms(date: &str) -> Option<(i64, i64)> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let next_date = date.succ_opt()?;
    let start = Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    let end = Local
        .from_local_datetime(&next_date.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    Some((start.timestamp_millis(), end.timestamp_millis()))
}

fn normalize_timestamp_ms(timestamp: i64) -> i64 {
    if timestamp > 0 && timestamp < 10_000_000_000 {
        timestamp * 1_000
    } else {
        timestamp
    }
}

fn first_i64(object: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| object.get(*key).map(to_i64))
        .filter(|value| *value > 0)
}

fn string_field(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn to_i64(value: &Value) -> i64 {
    if let Some(number) = value.as_i64() {
        return number;
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).unwrap_or_default();
    }
    if let Some(number) = value.as_f64() {
        return if number.is_finite() { number as i64 } else { 0 };
    }
    value
        .as_str()
        .and_then(|text| text.parse::<i64>().ok())
        .unwrap_or_default()
}

fn num(value: &Value) -> f64 {
    if let Some(number) = value.as_f64() {
        return if number.is_finite() { number } else { 0.0 };
    }
    value
        .as_str()
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|number| number.is_finite())
        .unwrap_or_default()
}

fn sort_key(value: &Value) -> String {
    let date = value
        .get("date")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let time = value
        .get("time")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("{date}T{time}")
}

fn current_timestamp_ms() -> i64 {
    Local::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn loads_and_aggregates_json_messages() {
        let _lock = ENV_LOCK.lock().unwrap();
        let root = temp_root();
        let message_dir = root.join("storage/message/a");
        let session_dir = root.join("storage/session");
        fs::create_dir_all(&message_dir).unwrap();
        fs::create_dir_all(&session_dir).unwrap();

        fs::write(
            message_dir.join("one.json"),
            r#"{
                "id": "msg_1",
                "sessionID": "ses_1",
                "providerID": "anthropic",
                "modelID": "claude-sonnet-4-5",
                "time": { "created": 1700000000000 },
                "tokens": { "input": 100, "output": 50, "cache": { "write": 10, "read": 20 } },
                "cost": 0.01
            }"#,
        )
        .unwrap();
        fs::write(
            message_dir.join("z_dupe.json"),
            r#"{
                "id": "msg_1",
                "sessionID": "ses_1",
                "providerID": "anthropic",
                "modelID": "claude-sonnet-4-5",
                "time": { "created": 1700000000000 },
                "tokens": { "input": 999, "output": 999 },
                "cost": 9
            }"#,
        )
        .unwrap();
        fs::write(
            message_dir.join("filtered.json"),
            r#"{
                "id": "msg_2",
                "sessionID": "ses_1",
                "providerID": "anthropic",
                "time": { "created": 1700000000000 },
                "tokens": { "input": 1, "output": 1 }
            }"#,
        )
        .unwrap();
        fs::write(
            session_dir.join("ses_1.json"),
            r#"{ "id": "ses_1", "title": "Planning" }"#,
        )
        .unwrap();

        with_opencode_dir(&root, || {
            let daily = load_source_view("daily", false).unwrap();
            assert_eq!(daily.len(), 1);
            assert_eq!(daily[0]["inputTokens"], 100);
            assert_eq!(daily[0]["outputTokens"], 50);
            assert_eq!(daily[0]["cacheCreationTokens"], 10);
            assert_eq!(daily[0]["cacheReadTokens"], 20);
            assert_eq!(daily[0]["totalTokens"], 180);
            assert_eq!(daily[0]["totalCost"], 0.01);

            let sessions = load_source_view("sessions", false).unwrap();
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0]["sessionId"], "ses_1");
            assert_eq!(sessions[0]["sessionTitle"], "Planning");
        });

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn blocks_view_is_empty() {
        let blocks = load_source_view("blocks", false).unwrap();
        assert!(blocks.is_empty());
    }

    fn with_opencode_dir(root: &Path, test: impl FnOnce()) {
        let original = std::env::var_os(OPENCODE_DATA_DIR_ENV);
        std::env::set_var(OPENCODE_DATA_DIR_ENV, root);
        test();
        if let Some(original) = original {
            std::env::set_var(OPENCODE_DATA_DIR_ENV, original);
        } else {
            std::env::remove_var(OPENCODE_DATA_DIR_ENV);
        }
    }

    fn temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("token-usage-opencode-test-{now}"))
    }
}
