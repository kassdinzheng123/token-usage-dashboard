use crate::pricing::{model_cost_usd, TokenUsage};
use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    convert::TryFrom,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const UNKNOWN_MODEL: &str = "unknown";
const FALLBACK_PRICING_MODEL: &str = "gpt-5";
const CODEX_HOMES_ENV: &str = "TOKEN_USAGE_CODEX_HOMES";
/// Max inter-row gap (ms) still treated as part of a forked rollout's replay
/// burst. Mirrors TokenTracker (`CODEX_FORK_REPLAY_GAP_MS`): replay flush spaces
/// rows sub-ms to a few ms apart, while live turns arrive multi-second apart.
const CODEX_FORK_REPLAY_GAP_MS: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceView {
    Daily,
    Monthly,
    Sessions,
    Blocks,
    Messages,
}

impl TryFrom<&str> for SourceView {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "daily" => Ok(Self::Daily),
            "monthly" => Ok(Self::Monthly),
            "sessions" => Ok(Self::Sessions),
            "blocks" => Ok(Self::Blocks),
            "messages" => Ok(Self::Messages),
            other => Err(format!("unsupported view: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RawUsage {
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone)]
struct TokenUsageEvent {
    session_id: String,
    date: String,
    time: String,
    timestamp_millis: i64,
    model_name: String,
    is_fallback_model: bool,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    cost: f64,
}

#[derive(Debug, Clone)]
struct PendingTokenUsageEvent {
    date: String,
    time: String,
    timestamp_millis: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    explicit_cost: f64,
}

#[derive(Debug, Clone, Default)]
struct ModelUsage {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cost: f64,
    has_non_fallback: bool,
}

#[derive(Debug, Clone, Default)]
struct UsageGroup {
    key: String,
    session_id: String,
    date: String,
    time: String,
    timestamp_millis: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    total_cost: f64,
    models: BTreeMap<String, ModelUsage>,
}

#[derive(Debug, Clone)]
struct SessionFile {
    codex_home: PathBuf,
    session_root: PathBuf,
    file: PathBuf,
}

pub fn load_source_view(view: &str, refresh: bool) -> Result<Vec<Value>, String> {
    load_source_view_since(view, refresh, None)
}

pub fn load_source_view_since(
    view: &str,
    _refresh: bool,
    watermark_ms: Option<i64>,
) -> Result<Vec<Value>, String> {
    let view = SourceView::try_from(view)?;

    let events = load_events(watermark_ms)?;
    Ok(match view {
        SourceView::Daily => events_to_period_rows(&events, PeriodView::Daily),
        SourceView::Monthly => events_to_period_rows(&events, PeriodView::Monthly),
        SourceView::Sessions => events_to_sessions(&events),
        SourceView::Blocks => events_to_blocks(&events),
        SourceView::Messages => events_to_messages(&events),
    })
}

pub fn load_daily_for_date(date: &str, refresh: bool) -> Result<Vec<Value>, String> {
    let _ = refresh;
    let events = load_events_for_date(date)?;
    Ok(events_to_period_rows(&events, PeriodView::Daily)
        .into_iter()
        .filter(|row| row.get("date").and_then(Value::as_str) == Some(date))
        .collect())
}

fn load_events(watermark_ms: Option<i64>) -> Result<Vec<TokenUsageEvent>, String> {
    let mut files = collect_session_files()?;
    let include_home_in_session_id = files
        .iter()
        .map(|file| file.codex_home.as_path())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1;

    let mut events = Vec::new();
    for session_file in files.drain(..) {
        if let Some(watermark) = watermark_ms {
            if !super::file_modified_after(&session_file.file, watermark) {
                continue;
            }
        }
        append_events_from_file(&session_file, include_home_in_session_id, &mut events)?;
    }

    events.sort_by_key(|event| event.timestamp_millis);
    Ok(events)
}

fn load_events_for_date(date: &str) -> Result<Vec<TokenUsageEvent>, String> {
    if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Ok(Vec::new());
    }

    let mut events = load_events(None)?;
    events.retain(|event| event.date == date);
    Ok(events)
}

fn collect_session_files() -> Result<Vec<SessionFile>, String> {
    let mut files_by_path = BTreeMap::new();

    for codex_home in codex_homes() {
        let session_roots = [
            codex_home.join("sessions"),
            codex_home.join("archived_sessions"),
        ];

        for session_root in session_roots {
            if !session_root.is_dir() {
                continue;
            }

            let mut root_files = Vec::new();
            collect_jsonl_files(&session_root, &mut root_files)?;
            for file in root_files {
                insert_session_file(
                    &mut files_by_path,
                    SessionFile {
                        codex_home: codex_home.clone(),
                        session_root: session_root.clone(),
                        file,
                    },
                );
            }
        }

        append_rollout_paths_from_state(&codex_home, &mut files_by_path);
    }

    Ok(files_by_path.into_values().collect())
}

fn insert_session_file(files_by_path: &mut BTreeMap<PathBuf, SessionFile>, file: SessionFile) {
    let key = file
        .file
        .canonicalize()
        .unwrap_or_else(|_| file.file.clone());
    files_by_path.entry(key).or_insert(file);
}

fn append_rollout_paths_from_state(
    codex_home: &Path,
    files_by_path: &mut BTreeMap<PathBuf, SessionFile>,
) {
    let Ok(entries) = fs::read_dir(codex_home) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("state_") || !name.ends_with(".sqlite") {
            continue;
        }
        append_rollout_paths_from_state_db(codex_home, &path, files_by_path);
    }
}

fn append_rollout_paths_from_state_db(
    codex_home: &Path,
    db_path: &Path,
    files_by_path: &mut BTreeMap<PathBuf, SessionFile>,
) {
    let Ok(connection) = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return;
    };
    let Ok(mut statement) = connection.prepare(
        "SELECT DISTINCT rollout_path FROM threads WHERE rollout_path IS NOT NULL AND rollout_path <> ''",
    ) else {
        return;
    };
    let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(0)) else {
        return;
    };

    for row in rows.flatten() {
        let raw_file = PathBuf::from(row);
        let file = if raw_file.is_absolute() {
            raw_file
        } else {
            codex_home.join(raw_file)
        };
        if !file.is_file() {
            continue;
        }
        insert_session_file(
            files_by_path,
            SessionFile {
                codex_home: codex_home.to_path_buf(),
                session_root: session_root_for_rollout_path(codex_home, &file),
                file,
            },
        );
    }
}

fn session_root_for_rollout_path(codex_home: &Path, file: &Path) -> PathBuf {
    for root in [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ] {
        if file.starts_with(&root) {
            return root;
        }
    }

    file.parent().unwrap_or(codex_home).to_path_buf()
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
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn append_events_from_file(
    session_file: &SessionFile,
    include_home_in_session_id: bool,
    events: &mut Vec<TokenUsageEvent>,
) -> Result<(), String> {
    let file = match File::open(&session_file.file) {
        Ok(file) => file,
        Err(_) => return Ok(()),
    };

    let session_id = session_id_for(session_file, include_home_in_session_id);
    let rollout_date = rollout_date_from_path(&session_file.file);
    let mut previous_totals_by_scope = BTreeMap::new();
    let mut current_model: Option<String> = None;
    let mut current_provider: Option<String> = None;
    let mut current_date: Option<String> = None;
    let mut is_forked_rollout = false;
    // Replay prefix only exists at the head of a freshly-forked file. Latch off
    // permanently at the first multi-second gap (TokenTracker issue #169).
    let mut replay_prefix_active = true;
    let mut prev_forked_token_ms: Option<i64> = None;
    let mut pending_without_model = Vec::new();

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(entry) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let Some(payload) = entry.get("payload").filter(|value| value.is_object()) else {
            continue;
        };

        if entry.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(provider) = extract_provider(payload) {
                current_provider = Some(provider);
            }
            if payload
                .get("forked_from_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|id| !id.is_empty())
            {
                is_forked_rollout = true;
            }
            continue;
        }

        if entry.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(provider) = extract_provider(payload) {
                current_provider = Some(provider);
            }
            if let Some(date) = payload
                .get("current_date")
                .and_then(Value::as_str)
                .and_then(normalize_iso_date)
            {
                current_date = Some(date);
            }
            if let Some(model) = extract_model(payload) {
                current_model = Some(model.clone());
                flush_pending_token_usage_events(
                    &mut pending_without_model,
                    events,
                    &session_id,
                    model,
                );
            }
            continue;
        }

        if entry.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }

        let total_usage = payload
            .get("info")
            .and_then(|info| info.get("total_token_usage"))
            .and_then(normalize_raw_usage);
        let last_usage = payload
            .get("info")
            .and_then(|info| info.get("last_token_usage"))
            .and_then(normalize_raw_usage);
        let extracted_model = extract_model(payload);
        let usage_scope = usage_scope(current_provider.as_deref(), payload);

        let raw_usage = match (last_usage, total_usage) {
            (Some(last), total) => {
                if let Some(total) = total {
                    previous_totals_by_scope.insert(usage_scope.clone(), total);
                }
                Some(last)
            }
            (None, Some(total)) => {
                let previous_totals = previous_totals_by_scope.get(&usage_scope).copied();
                let delta = subtract_raw_usage(total, previous_totals);
                previous_totals_by_scope.insert(usage_scope, total);
                Some(delta)
            }
            (None, None) => None,
        };

        let Some(raw_usage) = raw_usage else {
            continue;
        };
        if raw_usage.input_tokens == 0
            && raw_usage.cached_input_tokens == 0
            && raw_usage.output_tokens == 0
            && raw_usage.reasoning_output_tokens == 0
        {
            continue;
        }

        let Some(timestamp) = entry.get("timestamp").and_then(Value::as_str) else {
            continue;
        };
        let Some((date, time, timestamp_millis)) = timestamp_parts(timestamp) else {
            continue;
        };

        // Forked Codex rollouts replay the parent session's token history into
        // the child file at spawn time. Skip that replay (TokenTracker #169).
        // Totals above are still advanced so total-delta chains stay intact.
        let forked_replay_skip = forked_replay_burst_skip(
            is_forked_rollout,
            &mut replay_prefix_active,
            &mut prev_forked_token_ms,
            timestamp_millis,
        );
        if forked_replay_skip
            || is_forked_replay_token(is_forked_rollout, rollout_date.as_deref(), current_date.as_deref())
        {
            continue;
        }

        if let Some(model) = extracted_model.as_ref() {
            current_model = Some(model.clone());
            flush_pending_token_usage_events(
                &mut pending_without_model,
                events,
                &session_id,
                model.clone(),
            );
        }
        let cache_read_tokens = raw_usage
            .cached_input_tokens
            .min(raw_usage.input_tokens)
            .max(0);
        let pending_event = PendingTokenUsageEvent {
            date,
            time,
            timestamp_millis,
            input_tokens: raw_usage.input_tokens.saturating_sub(cache_read_tokens),
            output_tokens: raw_usage.output_tokens,
            cache_read_tokens,
            total_tokens: if raw_usage.total_tokens > 0 {
                raw_usage.total_tokens
            } else {
                raw_usage.input_tokens + raw_usage.output_tokens
            },
            explicit_cost: explicit_cost(payload),
        };

        if let Some(model_name) = extracted_model.or_else(|| current_model.clone()) {
            push_token_usage_event(events, &session_id, pending_event, model_name, false);
        } else {
            pending_without_model.push(pending_event);
        }
    }

    flush_pending_unknown_token_usage_events(&mut pending_without_model, events, &session_id);

    Ok(())
}

fn flush_pending_token_usage_events(
    pending_events: &mut Vec<PendingTokenUsageEvent>,
    events: &mut Vec<TokenUsageEvent>,
    session_id: &str,
    model_name: String,
) {
    for pending_event in pending_events.drain(..) {
        push_token_usage_event(events, session_id, pending_event, model_name.clone(), false);
    }
}

fn flush_pending_unknown_token_usage_events(
    pending_events: &mut Vec<PendingTokenUsageEvent>,
    events: &mut Vec<TokenUsageEvent>,
    session_id: &str,
) {
    for pending_event in pending_events.drain(..) {
        push_token_usage_event(
            events,
            session_id,
            pending_event,
            UNKNOWN_MODEL.to_string(),
            true,
        );
    }
}

fn push_token_usage_event(
    events: &mut Vec<TokenUsageEvent>,
    session_id: &str,
    pending_event: PendingTokenUsageEvent,
    model_name: String,
    is_fallback_model: bool,
) {
    let cost = if pending_event.explicit_cost > 0.0 {
        pending_event.explicit_cost
    } else {
        model_cost_usd(
            if is_fallback_model {
                FALLBACK_PRICING_MODEL
            } else {
                &model_name
            },
            TokenUsage {
                input_tokens: pending_event.input_tokens,
                output_tokens: pending_event.output_tokens,
                cache_creation_tokens: 0,
                cache_read_tokens: pending_event.cache_read_tokens,
            },
        )
    };

    events.push(TokenUsageEvent {
        session_id: session_id.to_string(),
        date: pending_event.date,
        time: pending_event.time,
        timestamp_millis: pending_event.timestamp_millis,
        model_name,
        is_fallback_model,
        input_tokens: pending_event.input_tokens,
        output_tokens: pending_event.output_tokens,
        cache_read_tokens: pending_event.cache_read_tokens,
        total_tokens: pending_event.total_tokens,
        cost,
    });
}

fn events_to_sessions(events: &[TokenUsageEvent]) -> Vec<Value> {
    aggregate_events(events, |event| event.session_id.clone())
        .into_iter()
        .map(|group| {
            json!({
                "sessionId": group.session_id,
                "date": group.date,
                "time": group.time,
                "inputTokens": group.input_tokens,
                "outputTokens": group.output_tokens,
                "cacheCreationTokens": 0,
                "cacheReadTokens": group.cache_read_tokens,
                "totalTokens": group.total_tokens,
                "totalCost": group.total_cost,
                "modelsUsed": models_used(&group),
                "modelBreakdowns": model_breakdowns(&group),
            })
        })
        .collect()
}

fn events_to_blocks(events: &[TokenUsageEvent]) -> Vec<Value> {
    events
        .iter()
        .map(|event| {
            let block_id = format!("{}-{}", event.session_id, event.timestamp_millis);
            json!({
                "blockId": block_id,
                "sessionId": event.session_id,
                "modelName": super::cluster_model_name_at(&event.model_name, Some(&event.date)),
                "timestamp": event.timestamp_millis,
                "date": event.date,
                "time": event.time,
                "inputTokens": event.input_tokens,
                "outputTokens": event.output_tokens,
                "cacheCreationTokens": 0,
                "cacheReadTokens": event.cache_read_tokens,
                "totalTokens": event.total_tokens,
                "cost": event.cost,
            })
        })
        .collect()
}

/// One row per token-usage event. Ids mirror the blocks view
/// (`session_id-timestamp_millis`) so ledger upserts stay idempotent.
fn events_to_messages(events: &[TokenUsageEvent]) -> Vec<Value> {
    events
        .iter()
        .map(|event| {
            let message_id = format!("{}-{}", event.session_id, event.timestamp_millis);
            json!({
                "messageId": message_id,
                "sessionId": event.session_id,
                "date": event.date,
                "time": event.time,
                "inputTokens": event.input_tokens,
                "outputTokens": event.output_tokens,
                "cacheCreationTokens": 0,
                "cacheReadTokens": event.cache_read_tokens,
                "totalTokens": event.total_tokens,
                "cost": event.cost,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum PeriodView {
    Daily,
    Monthly,
}

fn events_to_period_rows(events: &[TokenUsageEvent], view: PeriodView) -> Vec<Value> {
    let groups = aggregate_events(events, |event| match view {
        PeriodView::Daily => event.date.clone(),
        PeriodView::Monthly => event.date.chars().take(7).collect(),
    });
    groups
        .into_iter()
        .map(|group| match view {
            PeriodView::Daily => json!({
                "date": group.key,
                "inputTokens": group.input_tokens,
                "outputTokens": group.output_tokens,
                "cacheCreationTokens": 0,
                "cacheReadTokens": group.cache_read_tokens,
                "totalTokens": group.total_tokens,
                "totalCost": group.total_cost,
                "modelsUsed": models_used_from_model_map(&group.models),
                "modelBreakdowns": model_breakdowns_from_model_map(&group.models),
            }),
            PeriodView::Monthly => json!({
                "month": group.key,
                "inputTokens": group.input_tokens,
                "outputTokens": group.output_tokens,
                "cacheCreationTokens": 0,
                "cacheReadTokens": group.cache_read_tokens,
                "totalTokens": group.total_tokens,
                "totalCost": group.total_cost,
                "modelsUsed": models_used_from_model_map(&group.models),
                "modelBreakdowns": model_breakdowns_from_model_map(&group.models),
            }),
        })
        .collect()
}

fn models_used_from_model_map(models: &BTreeMap<String, ModelUsage>) -> Vec<String> {
    models
        .iter()
        .filter_map(|(model_name, usage)| usage.has_non_fallback.then(|| model_name.clone()))
        .collect()
}

fn model_breakdowns_from_model_map(models: &BTreeMap<String, ModelUsage>) -> Vec<Value> {
    models
        .iter()
        .map(|(model_name, usage)| {
            json!({
                "modelName": model_name,
                "inputTokens": usage.input_tokens,
                "outputTokens": usage.output_tokens,
                "cacheCreationTokens": 0,
                "cacheReadTokens": usage.cache_read_tokens,
                "cost": usage.cost,
            })
        })
        .collect()
}

fn aggregate_events(
    events: &[TokenUsageEvent],
    key_for: impl Fn(&TokenUsageEvent) -> String,
) -> Vec<UsageGroup> {
    let mut groups: BTreeMap<String, UsageGroup> = BTreeMap::new();

    for event in events {
        let key = key_for(event);
        let group = groups.entry(key.clone()).or_insert_with(|| UsageGroup {
            key,
            session_id: event.session_id.clone(),
            date: event.date.clone(),
            time: event.time.clone(),
            timestamp_millis: event.timestamp_millis,
            ..UsageGroup::default()
        });

        group.input_tokens += event.input_tokens;
        group.output_tokens += event.output_tokens;
        group.cache_read_tokens += event.cache_read_tokens;
        group.total_tokens += event.total_tokens;
        group.total_cost += event.cost;

        if event.timestamp_millis >= group.timestamp_millis {
            group.session_id = event.session_id.clone();
            group.date = event.date.clone();
            group.time = event.time.clone();
            group.timestamp_millis = event.timestamp_millis;
        }

        let model = group
            .models
            .entry(super::cluster_model_name_at(&event.model_name, Some(&event.date)))
            .or_insert_with(ModelUsage::default);
        model.input_tokens += event.input_tokens;
        model.output_tokens += event.output_tokens;
        model.cache_read_tokens += event.cache_read_tokens;
        model.cost += event.cost;
        if !event.is_fallback_model {
            model.has_non_fallback = true;
        }
    }

    groups.into_values().collect()
}

fn model_breakdowns(group: &UsageGroup) -> Vec<Value> {
    group
        .models
        .iter()
        .map(|(model_name, usage)| {
            json!({
                "modelName": model_name,
                "inputTokens": usage.input_tokens,
                "outputTokens": usage.output_tokens,
                "cacheCreationTokens": 0,
                "cacheReadTokens": usage.cache_read_tokens,
                "cost": usage.cost,
            })
        })
        .collect()
}

fn models_used(group: &UsageGroup) -> Vec<String> {
    group
        .models
        .iter()
        .filter_map(|(model_name, usage)| usage.has_non_fallback.then(|| model_name.clone()))
        .collect()
}

fn normalize_raw_usage(value: &Value) -> Option<RawUsage> {
    if !value.is_object() {
        return None;
    }

    let input_tokens = value.get("input_tokens").map(to_i64).unwrap_or_default();
    let cached_input_tokens = value
        .get("cached_input_tokens")
        .or_else(|| value.get("cache_read_input_tokens"))
        .map(to_i64)
        .unwrap_or_default();
    let output_tokens = value.get("output_tokens").map(to_i64).unwrap_or_default();
    let reasoning_output_tokens = value
        .get("reasoning_output_tokens")
        .map(to_i64)
        .unwrap_or_default();
    let provided_total_tokens = value.get("total_tokens").map(to_i64).unwrap_or_default();
    let total_tokens = if provided_total_tokens > 0 {
        provided_total_tokens
    } else {
        input_tokens + output_tokens
    };

    Some(RawUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        total_tokens,
    })
}

fn subtract_raw_usage(current: RawUsage, previous: Option<RawUsage>) -> RawUsage {
    let previous = previous.unwrap_or_default();
    if current.total_tokens < previous.total_tokens {
        return current;
    }
    RawUsage {
        input_tokens: (current.input_tokens - previous.input_tokens).max(0),
        cached_input_tokens: (current.cached_input_tokens - previous.cached_input_tokens).max(0),
        output_tokens: (current.output_tokens - previous.output_tokens).max(0),
        reasoning_output_tokens: (current.reasoning_output_tokens
            - previous.reasoning_output_tokens)
            .max(0),
        total_tokens: (current.total_tokens - previous.total_tokens).max(0),
    }
}

fn extract_model(payload: &Value) -> Option<String> {
    let info = payload.get("info");
    let candidates = [
        info.and_then(|info| info.get("model")),
        info.and_then(|info| info.get("model_name")),
        info.and_then(|info| info.get("metadata"))
            .and_then(|metadata| metadata.get("model")),
        payload.get("model"),
        payload
            .get("metadata")
            .and_then(|metadata| metadata.get("model")),
    ];

    candidates
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|model| !model.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_provider(payload: &Value) -> Option<String> {
    let candidates = [
        payload.get("model_provider"),
        payload.get("provider"),
        payload.get("provider_id"),
        payload.get("modelProvider"),
        payload
            .get("metadata")
            .and_then(|metadata| metadata.get("model_provider")),
        payload
            .get("metadata")
            .and_then(|metadata| metadata.get("provider")),
    ];

    candidates
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|provider| !provider.is_empty())
        .map(ToOwned::to_owned)
}

fn usage_scope(current_provider: Option<&str>, payload: &Value) -> String {
    payload
        .get("rate_limits")
        .and_then(|rate_limits| rate_limits.get("limit_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .or(current_provider)
        .unwrap_or("default-provider")
        .to_string()
}

fn explicit_cost(payload: &Value) -> f64 {
    let candidates = [
        payload.get("cost"),
        payload.get("costUSD"),
        payload.get("totalCost"),
        payload.get("total_cost"),
        payload.get("info").and_then(|info| info.get("cost")),
        payload.get("info").and_then(|info| info.get("costUSD")),
        payload.get("info").and_then(|info| info.get("totalCost")),
        payload.get("info").and_then(|info| info.get("total_cost")),
    ];

    candidates
        .into_iter()
        .flatten()
        .map(to_f64)
        .find(|cost| *cost > 0.0)
        .unwrap_or_default()
}

fn timestamp_parts(timestamp: &str) -> Option<(String, String, i64)> {
    let date_time = DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&Local);
    Some((
        date_time.format("%Y-%m-%d").to_string(),
        date_time.format("%H:%M").to_string(),
        date_time.timestamp_millis(),
    ))
}

/// Extract `YYYY-MM-DD` from Codex rollout filenames like
/// `rollout-2026-06-09T20-46-23-<uuid>.jsonl` (TokenTracker `rolloutDateFromPath`).
fn rollout_date_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix("rollout-")?;
    if rest.len() < 10 {
        return None;
    }
    let date = &rest[..10];
    if date.as_bytes().get(4) != Some(&b'-') || date.as_bytes().get(7) != Some(&b'-') {
        return None;
    }
    if !rest.as_bytes().get(10).is_some_and(|b| *b == b'T') {
        return None;
    }
    if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return None;
    }
    Some(date.to_string())
}

fn normalize_iso_date(value: &str) -> Option<String> {
    let raw = value.trim();
    if raw.len() < 10 {
        return None;
    }
    let date = &raw[..10];
    if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return None;
    }
    Some(date.to_string())
}

/// Cross-day fork guard: replayed history carries an older `current_date` than
/// the child rollout's filename date.
fn is_forked_replay_token(
    is_forked_rollout: bool,
    rollout_date: Option<&str>,
    current_date: Option<&str>,
) -> bool {
    matches!(
        (is_forked_rollout, rollout_date, current_date),
        (true, Some(rollout), Some(current)) if current < rollout
    )
}

/// Same-day fork burst detector. Skips the *leading* densely-spaced
/// `token_count` rows in a forked rollout; latches off at the first gap ≥
/// `CODEX_FORK_REPLAY_GAP_MS`. The first row of the burst is still counted
/// (no lookahead); backwards/unparseable clocks fail open.
fn forked_replay_burst_skip(
    is_forked_rollout: bool,
    replay_prefix_active: &mut bool,
    prev_forked_token_ms: &mut Option<i64>,
    token_ms: i64,
) -> bool {
    if !is_forked_rollout || !*replay_prefix_active {
        return false;
    }

    if let Some(prev) = *prev_forked_token_ms {
        if token_ms < prev {
            // Fail open on backwards clock steps.
            *replay_prefix_active = false;
            return false;
        }
        if token_ms - prev >= CODEX_FORK_REPLAY_GAP_MS {
            *replay_prefix_active = false;
        }
    }

    let skip = *replay_prefix_active
        && prev_forked_token_ms.is_some_and(|prev| token_ms - prev < CODEX_FORK_REPLAY_GAP_MS);
    *prev_forked_token_ms = Some(token_ms);
    skip
}

fn session_id_for(session_file: &SessionFile, include_home: bool) -> String {
    let relative = session_file
        .file
        .strip_prefix(&session_file.session_root)
        .unwrap_or(&session_file.file);
    let without_extension = relative.with_extension("");
    let session_id = without_extension
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");

    if include_home {
        format!(
            "{}/{}",
            codex_home_label(&session_file.codex_home),
            session_id
        )
    } else {
        session_id
    }
}

fn codex_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();

    if let Some(value) = std::env::var_os(CODEX_HOMES_ENV) {
        for path in split_path_list(&value.to_string_lossy()) {
            push_codex_home(&mut homes, expand_home_path(path));
        }
        return existing_codex_homes(homes);
    }

    if let Some(path) = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        push_codex_home(&mut homes, path);
        return existing_codex_homes(homes);
    }

    if let Some(home) = super::home_dir() {
        push_codex_home(&mut homes, home.join(".codex"));
        discover_codex_homes_in(&home, |name| name.starts_with(".codex"), &mut homes);
        discover_codex_homes_in(
            &home.join(".local").join("share"),
            contains_codex,
            &mut homes,
        );
        discover_codex_homes_in(
            &home.join("Library").join("Application Support"),
            contains_codex,
            &mut homes,
        );
    }

    existing_codex_homes(homes)
}

fn existing_codex_homes(mut homes: Vec<PathBuf>) -> Vec<PathBuf> {
    homes.retain(|home| has_codex_storage(home));
    homes.sort();
    homes.dedup();
    homes
}

fn has_codex_storage(home: &Path) -> bool {
    if home.join("sessions").is_dir() || home.join("archived_sessions").is_dir() {
        return true;
    }

    let Ok(entries) = fs::read_dir(home) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("state_") && name.ends_with(".sqlite"))
    })
}

fn push_codex_home(homes: &mut Vec<PathBuf>, path: PathBuf) {
    let canonical = path.canonicalize().unwrap_or(path);
    if !homes.contains(&canonical) {
        homes.push(canonical);
    }
}

fn discover_codex_homes_in(
    dir: &Path,
    matches_name: impl Fn(&str) -> bool,
    homes: &mut Vec<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if matches_name(name) {
            push_codex_home(homes, path);
        }
    }
}

fn contains_codex(name: &str) -> bool {
    name.to_ascii_lowercase().contains("codex")
}

fn split_path_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([',', ':'])
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

fn expand_home_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = super::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

fn codex_home_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("codex")
        .to_string()
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

fn to_f64(value: &Value) -> f64 {
    if let Some(number) = value.as_f64() {
        return if number.is_finite() { number } else { 0.0 };
    }
    value
        .as_str()
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|number| number.is_finite())
        .unwrap_or_default()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::{
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    pub static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn watermark_skips_unchanged_files_and_rereads_newer_ones() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session(
            "project/session-a.jsonl",
            &[json!({
                "timestamp": "2025-09-11T18:25:40.670Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "model_name": "gpt-5-codex",
                        "total_token_usage": {
                            "input_tokens": 120,
                            "cached_input_tokens": 20,
                            "output_tokens": 50,
                            "total_tokens": 170
                        },
                        "last_token_usage": {
                            "input_tokens": 120,
                            "cached_input_tokens": 20,
                            "output_tokens": 50,
                            "total_tokens": 170
                        }
                    }
                }
            })],
        );
        let session_file = fixture.root.join("sessions").join("project/session-a.jsonl");
        let mtime_ms = file_mtime_ms(&session_file);

        // First run (no watermark): full scan.
        assert_eq!(
            load_source_view_since("sessions", true, None).unwrap().len(),
            1
        );
        // Unchanged file (mtime at the watermark): skipped.
        assert!(load_source_view_since("sessions", true, Some(mtime_ms))
            .unwrap()
            .is_empty());
        // File newer than the watermark: re-read.
        assert_eq!(
            load_source_view_since("sessions", true, Some(mtime_ms - 1))
                .unwrap()
                .len(),
            1
        );
    }

    fn file_mtime_ms(path: &Path) -> i64 {
        let modified = fs::metadata(path).unwrap().modified().unwrap();
        i64::try_from(modified.duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap()
    }

    #[test]
    fn loads_daily_with_last_usage_and_total_usage_delta() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session(
            "project/session-a.jsonl",
            &[
                json!({
                    "timestamp": "2025-09-11T18:25:40.670Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "cost": 0.01,
                        "info": {
                            "model_name": "gpt-5-codex",
                            "total_token_usage": {
                                "input_tokens": 120,
                                "cached_input_tokens": 20,
                                "output_tokens": 50,
                                "total_tokens": 170
                            },
                            "last_token_usage": {
                                "input_tokens": 120,
                                "cached_input_tokens": 20,
                                "output_tokens": 50,
                                "total_tokens": 170
                            }
                        }
                    }
                }),
                json!({
                    "timestamp": "2025-09-11T18:40:00.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "metadata": { "model": "gpt-5-mini" },
                        "info": {
                            "total_token_usage": {
                                "input_tokens": 200,
                                "cached_input_tokens": 35,
                                "output_tokens": 80,
                                "total_tokens": 280
                            }
                        }
                    }
                }),
            ],
        );

        let rows = load_source_view("daily", false).unwrap();
        let expected_date = timestamp_parts("2025-09-11T18:25:40.670Z").unwrap().0;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["date"], expected_date);
        // inputTokens is now non-cached: event1(120-20) + event2(80-15) = 100+65 = 165
        assert_eq!(rows[0]["inputTokens"], 165);
        // outputTokens unchanged: 50+30 = 80
        assert_eq!(rows[0]["outputTokens"], 80);
        assert_eq!(rows[0]["cacheCreationTokens"], 0);
        // cacheReadTokens unchanged: 20+15 = 35
        assert_eq!(rows[0]["cacheReadTokens"], 35);
        // totalTokens unchanged: 170+110 = 280
        assert_eq!(rows[0]["totalTokens"], 280);
        assert_eq!(rows[0]["modelsUsed"], json!(["gpt-5-codex", "gpt-5-mini"]));
    }

    #[test]
    fn splits_cross_day_session_usage_by_event_date() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let first_timestamp = "2026-06-01T12:00:00.000Z";
        let second_timestamp = "2026-06-02T12:00:00.000Z";
        let first_date = timestamp_parts(first_timestamp).unwrap().0;
        let second_date = timestamp_parts(second_timestamp).unwrap().0;
        assert_ne!(first_date, second_date);

        fixture.write_session(
            "cross-day.jsonl",
            &[
                json!({
                    "timestamp": first_timestamp,
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "model_name": "gpt-5-codex",
                            "total_token_usage": {
                                "input_tokens": 120,
                                "cached_input_tokens": 20,
                                "output_tokens": 50,
                                "total_tokens": 170
                            },
                            "last_token_usage": {
                                "input_tokens": 120,
                                "cached_input_tokens": 20,
                                "output_tokens": 50,
                                "total_tokens": 170
                            }
                        }
                    }
                }),
                json!({
                    "timestamp": second_timestamp,
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "model_name": "gpt-5-codex",
                            "total_token_usage": {
                                "input_tokens": 200,
                                "cached_input_tokens": 35,
                                "output_tokens": 80,
                                "total_tokens": 280
                            }
                        }
                    }
                }),
            ],
        );

        let daily = load_source_view("daily", false).unwrap();
        assert_eq!(daily.len(), 2);
        assert_eq!(daily[0]["date"], first_date);
        assert_eq!(daily[0]["inputTokens"], 100);
        assert_eq!(daily[0]["outputTokens"], 50);
        assert_eq!(daily[0]["cacheReadTokens"], 20);
        assert_eq!(daily[0]["totalTokens"], 170);
        assert_eq!(daily[1]["date"], second_date);
        assert_eq!(daily[1]["inputTokens"], 65);
        assert_eq!(daily[1]["outputTokens"], 30);
        assert_eq!(daily[1]["cacheReadTokens"], 15);
        assert_eq!(daily[1]["totalTokens"], 110);

        let sessions = load_source_view("sessions", false).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["date"], second_date);
        assert_eq!(sessions[0]["inputTokens"], 165);
        assert_eq!(sessions[0]["outputTokens"], 80);
        assert_eq!(sessions[0]["cacheReadTokens"], 35);
        assert_eq!(sessions[0]["totalTokens"], 280);
    }

    #[test]
    fn keeps_fallback_model_out_of_models_used() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session(
            "legacy.jsonl",
            &[json!({
                "timestamp": "2025-09-15T13:00:00.000Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "total_token_usage": {
                            "input_tokens": 500,
                            "cached_input_tokens": 0,
                            "output_tokens": 100,
                            "total_tokens": 600
                        }
                    }
                }
            })],
        );

        let rows = load_source_view("sessions", false).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sessionId"], "legacy");
        assert_eq!(rows[0]["modelsUsed"], json!([]));
        assert_eq!(rows[0]["modelBreakdowns"][0]["modelName"], UNKNOWN_MODEL);
    }

    #[test]
    fn applies_turn_context_model_to_token_count_events() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session(
            "context-model.jsonl",
            &[
                json!({
                    "timestamp": "2026-05-06T03:34:29.958Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5"
                    }
                }),
                json!({
                    "timestamp": "2026-05-06T03:34:58.171Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 50,
                                "output_tokens": 20,
                                "reasoning_output_tokens": 5,
                                "total_tokens": 120
                            }
                        }
                    }
                }),
            ],
        );

        let rows = load_source_view("sessions", false).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["modelsUsed"], json!(["gpt-5.5"]));
        assert_eq!(rows[0]["modelBreakdowns"][0]["modelName"], "gpt-5.5");
    }

    #[test]
    fn counts_total_usage_after_provider_model_switch_resets_totals() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session(
            "provider-switch.jsonl",
            &[
                json!({
                    "timestamp": "2026-06-06T10:00:00.000Z",
                    "type": "session_meta",
                    "payload": {
                        "model_provider": "openai"
                    }
                }),
                json!({
                    "timestamp": "2026-06-06T10:00:01.000Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5"
                    }
                }),
                json!({
                    "timestamp": "2026-06-06T10:00:02.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "total_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 20,
                                "output_tokens": 30,
                                "total_tokens": 130
                            }
                        }
                    }
                }),
                json!({
                    "timestamp": "2026-06-06T10:00:03.000Z",
                    "type": "turn_context",
                    "payload": {
                        "model_provider": "custom",
                        "model": "deepseek-v4-flash"
                    }
                }),
                json!({
                    "timestamp": "2026-06-06T10:00:04.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "total_token_usage": {
                                "input_tokens": 50,
                                "cached_input_tokens": 10,
                                "output_tokens": 15,
                                "total_tokens": 65
                            }
                        }
                    }
                }),
            ],
        );

        let rows = load_source_view("sessions", false).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["inputTokens"], 120);
        assert_eq!(rows[0]["outputTokens"], 45);
        assert_eq!(rows[0]["cacheReadTokens"], 30);
        assert_eq!(rows[0]["totalTokens"], 195);
        assert_eq!(
            rows[0]["modelsUsed"],
            json!(["deepseek-v4-flash", "gpt-5.5"])
        );
    }

    #[test]
    fn loads_sessions_from_multiple_codex_homes() {
        let _guard = ENV_LOCK.lock().unwrap();
        let first = TestCodexHome::new_without_env("token-usage-codex-first");
        let second = TestCodexHome::new_without_env("token-usage-codex-second");
        let previous_codex_home = std::env::var_os("CODEX_HOME");
        let previous_codex_homes = std::env::var_os(CODEX_HOMES_ENV);
        std::env::remove_var("CODEX_HOME");
        std::env::set_var(
            CODEX_HOMES_ENV,
            format!("{}:{}", first.root.display(), second.root.display()),
        );

        first.write_session(
            "shared-name.jsonl",
            &[
                json!({
                    "timestamp": "2026-06-06T10:00:01.000Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5"
                    }
                }),
                json!({
                    "timestamp": "2026-06-06T10:00:02.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 20,
                                "output_tokens": 30,
                                "total_tokens": 130
                            }
                        }
                    }
                }),
            ],
        );
        second.write_session(
            "shared-name.jsonl",
            &[
                json!({
                    "timestamp": "2026-06-06T11:00:01.000Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "deepseek-v4-flash"
                    }
                }),
                json!({
                    "timestamp": "2026-06-06T11:00:02.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": 50,
                                "cached_input_tokens": 10,
                                "output_tokens": 15,
                                "total_tokens": 65
                            }
                        }
                    }
                }),
            ],
        );

        let daily = load_source_view("daily", false).unwrap();
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0]["inputTokens"], 120);
        assert_eq!(daily[0]["outputTokens"], 45);
        assert_eq!(daily[0]["cacheReadTokens"], 30);
        assert_eq!(daily[0]["totalTokens"], 195);
        assert_eq!(
            daily[0]["modelsUsed"],
            json!(["deepseek-v4-flash", "gpt-5.5"])
        );

        let sessions = load_source_view("sessions", false).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|row| row["sessionId"]
            .as_str()
            .is_some_and(|id| id.starts_with("token-usage-codex-first-"))));
        assert!(sessions.iter().any(|row| row["sessionId"]
            .as_str()
            .is_some_and(|id| id.starts_with("token-usage-codex-second-"))));

        restore_env("CODEX_HOME", previous_codex_home);
        restore_env(CODEX_HOMES_ENV, previous_codex_homes);
    }

    #[test]
    fn ignores_removed_dashboard_usage_cache_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let removed_cache_path = fixture.root.join("removed-dashboard-cache.json");

        fixture.write_session(
            "current.jsonl",
            &[
                json!({
                    "timestamp": "2026-06-06T10:00:01.000Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5"
                    }
                }),
                json!({
                    "timestamp": "2026-06-06T10:00:02.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 20,
                                "output_tokens": 30,
                                "total_tokens": 130
                            }
                        }
                    }
                }),
            ],
        );
        fs::write(
            &removed_cache_path,
            json!({
                "version": 1,
                "savedAt": "2026-04-30T14:31:14.884Z",
                "data": {
                    "codex:sessions": [{
                        "sessionId": "2026/04/30/rollout-old",
                        "date": "2026-04-30",
                        "time": "22:30",
                        "inputTokens": 200,
                        "outputTokens": 40,
                        "cacheCreationTokens": 0,
                        "cacheReadTokens": 50,
                        "totalTokens": 240,
                        "totalCost": 0.25,
                        "modelsUsed": ["gpt-5.4"],
                        "modelBreakdowns": [{
                            "modelName": "gpt-5.4",
                            "inputTokens": 200,
                            "outputTokens": 40,
                            "cacheCreationTokens": 0,
                            "cacheReadTokens": 50,
                            "cost": 0.25
                        }]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();

        let sessions = load_source_view("sessions", false).unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions
            .iter()
            .any(|row| row.get("sessionId").and_then(Value::as_str) == Some("current")));

        let daily = load_source_view("daily", false).unwrap();
        assert_eq!(daily.len(), 1);
        assert!(daily
            .iter()
            .any(
                |row| row.get("date").and_then(Value::as_str) == Some("2026-06-06")
                    && row.get("totalTokens").and_then(Value::as_i64) == Some(130)
            ));
    }

    #[test]
    fn loads_rollout_paths_from_codex_state() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let rollout_path = fixture
            .root
            .join("provider-state-rollouts")
            .join("state-only.jsonl");
        fs::create_dir_all(rollout_path.parent().unwrap()).unwrap();
        fs::write(
            &rollout_path,
            [
                json!({
                    "timestamp": "2026-06-07T10:00:01.000Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5"
                    }
                })
                .to_string(),
                json!({
                    "timestamp": "2026-06-07T10:00:02.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 20,
                                "output_tokens": 30,
                                "total_tokens": 130
                            }
                        }
                    }
                })
                .to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let connection = Connection::open(fixture.root.join("state_5.sqlite")).unwrap();
        connection
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO threads (id, rollout_path) VALUES ('thread-1', ?1)",
                [rollout_path.to_string_lossy().as_ref()],
            )
            .unwrap();

        let sessions = load_source_view("sessions", false).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["sessionId"], "state-only");
        assert_eq!(sessions[0]["inputTokens"], 80);
        assert_eq!(sessions[0]["outputTokens"], 30);
        assert_eq!(sessions[0]["cacheReadTokens"], 20);
        assert_eq!(sessions[0]["totalTokens"], 130);
    }

    #[test]
    fn applies_first_later_model_to_initial_token_count_before_turn_context() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session(
            "initial-token-count.jsonl",
            &[
                json!({
                    "timestamp": "2026-05-06T03:34:28.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 25,
                                "output_tokens": 20,
                                "total_tokens": 120
                            }
                        }
                    }
                }),
                json!({
                    "timestamp": "2026-05-06T03:34:29.958Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5"
                    }
                }),
            ],
        );

        let rows = load_source_view("sessions", false).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["modelsUsed"], json!(["gpt-5.5"]));
        assert_eq!(rows[0]["modelBreakdowns"][0]["modelName"], "gpt-5.5");
        assert_eq!(rows[0]["modelBreakdowns"][0]["inputTokens"], 75);
        assert_eq!(rows[0]["modelBreakdowns"][0]["outputTokens"], 20);
        assert_eq!(rows[0]["modelBreakdowns"][0]["cacheReadTokens"], 25);
    }

    #[test]
    fn loads_daily_for_date_from_resumed_older_session_directory() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let timestamp = "2026-05-09T02:47:18.216Z";
        let expected_date = timestamp_parts(timestamp).unwrap().0;
        fixture.write_session(
            "2026/01/01/resumed-old-thread.jsonl",
            &[
                json!({
                    "timestamp": "2026-05-09T02:47:15.183Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.4"
                    }
                }),
                json!({
                    "timestamp": timestamp,
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": 120,
                                "cached_input_tokens": 40,
                                "output_tokens": 20,
                                "total_tokens": 140
                            }
                        }
                    }
                }),
            ],
        );

        let rows = load_daily_for_date(&expected_date, false).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["date"], expected_date);
        assert_eq!(rows[0]["modelsUsed"], json!(["gpt-5.4"]));
        assert_eq!(rows[0]["modelBreakdowns"][0]["modelName"], "gpt-5.4");
    }

    #[test]
    fn loads_archived_sessions() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let timestamp = "2026-05-10T02:47:18.216Z";
        let expected_date = timestamp_parts(timestamp).unwrap().0;
        fixture.write_archived_session(
            "rollout-2026-05-10T02-47-18-archived.jsonl",
            &[
                json!({
                    "timestamp": "2026-05-10T02:47:15.183Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5"
                    }
                }),
                json!({
                    "timestamp": timestamp,
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": {
                                "input_tokens": 90,
                                "cached_input_tokens": 30,
                                "output_tokens": 10,
                                "total_tokens": 100
                            }
                        }
                    }
                }),
            ],
        );

        let rows = load_daily_for_date(&expected_date, false).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["date"], expected_date);
        assert_eq!(rows[0]["inputTokens"], 60);
        assert_eq!(rows[0]["outputTokens"], 10);
        assert_eq!(rows[0]["cacheReadTokens"], 30);
        assert_eq!(rows[0]["totalTokens"], 100);
        assert_eq!(rows[0]["modelsUsed"], json!(["gpt-5.5"]));
    }

    #[test]
    fn returns_empty_blocks_view() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session("ignored.jsonl", &[]);

        let rows = load_source_view("blocks", true).unwrap();
        assert!(rows.is_empty(), "blocks from empty session should be empty");
    }

    #[test]
    fn blocks_view_returns_per_event_rows_with_event_dates() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session(
            "cross-day.jsonl",
            &[
                json!({
                    "timestamp": "2026-06-01T12:00:00.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "model_name": "gpt-5-codex",
                            "total_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 20,
                                "output_tokens": 10,
                                "total_tokens": 110
                            },
                            "last_token_usage": {
                                "input_tokens": 100,
                                "cached_input_tokens": 20,
                                "output_tokens": 10,
                                "total_tokens": 110
                            }
                        }
                    }
                }),
                json!({
                    "timestamp": "2026-06-02T12:00:00.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "model_name": "gpt-5-codex",
                            "total_token_usage": {
                                "input_tokens": 200,
                                "cached_input_tokens": 40,
                                "output_tokens": 20,
                                "total_tokens": 220
                            }
                        }
                    }
                }),
            ],
        );

        let rows = load_source_view("blocks", true).unwrap();
        assert_eq!(rows.len(), 2, "should have one block per token_count event");

        let first = &rows[0];
        assert_eq!(first["date"], "2026-06-01", "first block date should be event date");
        assert_eq!(first["totalTokens"], 110);
        assert_eq!(first["cacheReadTokens"], 20);
        assert_eq!(first["inputTokens"], 80);

        let second = &rows[1];
        assert_eq!(second["date"], "2026-06-02", "second block date should be event date");
        // Second event has no last_token_usage, so delta is computed from total:
        // delta_input = 200 - 100 = 100, delta_cached = 40 - 20 = 20
        // cache_read = min(20, 100) = 20, input = 100 - 20 = 80
        assert_eq!(second["totalTokens"], 110);
        assert_eq!(second["cacheReadTokens"], 20);
        assert_eq!(second["inputTokens"], 80);
    }

    #[test]
    fn skips_historical_replay_tokens_in_cross_day_forked_rollouts() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let replay = json!({
            "input_tokens": 100,
            "cached_input_tokens": 0,
            "output_tokens": 10,
            "reasoning_output_tokens": 0,
            "total_tokens": 110
        });
        let live = json!({
            "input_tokens": 7,
            "cached_input_tokens": 0,
            "output_tokens": 3,
            "reasoning_output_tokens": 0,
            "total_tokens": 10
        });
        let live_totals = json!({
            "input_tokens": 107,
            "cached_input_tokens": 0,
            "output_tokens": 13,
            "reasoning_output_tokens": 0,
            "total_tokens": 120
        });

        fixture.write_session(
            "2026/06/09/rollout-2026-06-09T20-46-23-fork.jsonl",
            &[
                json!({
                    "timestamp": "2026-06-09T20:46:23.000Z",
                    "type": "session_meta",
                    "payload": {
                        "forked_from_id": "019e095c-c041-7b40-b7cb-43ddb153086c",
                        "model": "gpt-5.5"
                    }
                }),
                json!({
                    "timestamp": "2026-06-09T20:46:23.000Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5",
                        "current_date": "2026-05-08"
                    }
                }),
                json!({
                    "timestamp": "2026-06-09T20:46:26.530Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": replay,
                            "total_token_usage": replay
                        }
                    }
                }),
                json!({
                    "timestamp": "2026-06-09T20:47:00.000Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.5",
                        "current_date": "2026-06-09"
                    }
                }),
                json!({
                    "timestamp": "2026-06-09T20:47:00.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "last_token_usage": live,
                            "total_token_usage": live_totals
                        }
                    }
                }),
            ],
        );

        let rows = load_source_view("blocks", false).unwrap();
        assert_eq!(rows.len(), 1, "cross-day replay row must be skipped");
        assert_eq!(rows[0]["inputTokens"], 7);
        assert_eq!(rows[0]["outputTokens"], 3);
        assert_eq!(rows[0]["totalTokens"], 10);
    }

    #[test]
    fn skips_same_day_forked_replay_burst() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        // Three replay rows ~1ms apart, then one live turn 30s later.
        // current_date matches rollout date so only the burst detector applies.
        let r0 = usage(100, 10);
        let r1 = usage(200, 20);
        let r2 = usage(300, 30);
        let live = usage(7, 3);

        fixture.write_session(
            "rollout-2026-06-09T20-46-23-fork.jsonl",
            &[
                session_meta_forked("gpt-5.5"),
                turn_context("gpt-5.5", "2026-06-09"),
                token_count("2026-06-09T20:46:23.100Z", &r0, &cum(&[&r0])),
                token_count("2026-06-09T20:46:23.101Z", &r1, &cum(&[&r0, &r1])),
                token_count("2026-06-09T20:46:23.102Z", &r2, &cum(&[&r0, &r1, &r2])),
                token_count(
                    "2026-06-09T20:46:53.102Z",
                    &live,
                    &cum(&[&r0, &r1, &r2, &live]),
                ),
            ],
        );

        let rows = load_source_view("blocks", false).unwrap();
        // First-of-burst counted (no lookahead) + live; middle burst skipped.
        assert_eq!(rows.len(), 2);
        let total: i64 = rows
            .iter()
            .map(|row| row["totalTokens"].as_i64().unwrap_or(0))
            .sum();
        assert_eq!(total, 110 + 10);
    }

    #[test]
    fn latches_off_after_replay_burst_keeping_fast_live_turns() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let r0 = usage(100, 10);
        let r1 = usage(200, 20);
        let l0 = usage(5, 5);
        let l1 = usage(6, 6); // 120ms after l0 — must NOT be dropped

        fixture.write_session(
            "rollout-2026-06-09T20-46-23-fork.jsonl",
            &[
                session_meta_forked("gpt-5.5"),
                turn_context("gpt-5.5", "2026-06-09"),
                token_count("2026-06-09T20:46:23.100Z", &r0, &cum(&[&r0])),
                token_count("2026-06-09T20:46:23.101Z", &r1, &cum(&[&r0, &r1])),
                token_count("2026-06-09T20:46:53.000Z", &l0, &cum(&[&r0, &r1, &l0])),
                token_count(
                    "2026-06-09T20:46:53.120Z",
                    &l1,
                    &cum(&[&r0, &r1, &l0, &l1]),
                ),
            ],
        );

        let rows = load_source_view("blocks", false).unwrap();
        assert_eq!(rows.len(), 3);
        let total: i64 = rows
            .iter()
            .map(|row| row["totalTokens"].as_i64().unwrap_or(0))
            .sum();
        assert_eq!(total, 110 + 10 + 12);
    }

    #[test]
    fn fails_open_when_forked_timestamps_step_backwards() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let r0 = usage(100, 10);
        let l0 = usage(5, 5);
        let l1 = usage(6, 6);

        fixture.write_session(
            "rollout-2026-06-09T20-46-23-fork.jsonl",
            &[
                session_meta_forked("gpt-5.5"),
                turn_context("gpt-5.5", "2026-06-09"),
                token_count("2026-06-09T20:46:23.100Z", &r0, &cum(&[&r0])),
                token_count("2026-06-09T20:46:22.900Z", &l0, &cum(&[&r0, &l0])),
                token_count("2026-06-09T20:46:23.000Z", &l1, &cum(&[&r0, &l0, &l1])),
            ],
        );

        let rows = load_source_view("blocks", false).unwrap();
        assert_eq!(rows.len(), 3, "backwards clock must fail open and count all rows");
    }

    #[test]
    fn never_drops_genuine_turns_in_replay_free_fork() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let a = usage(50, 5);
        let b = usage(60, 6);

        fixture.write_session(
            "rollout-2026-06-09T20-46-23-fork.jsonl",
            &[
                session_meta_forked("gpt-5.5"),
                turn_context("gpt-5.5", "2026-06-09"),
                token_count("2026-06-09T20:46:28.000Z", &a, &a),
                token_count(
                    "2026-06-09T20:46:33.000Z",
                    &b,
                    &json!({
                        "input_tokens": 110,
                        "cached_input_tokens": 0,
                        "output_tokens": 11,
                        "reasoning_output_tokens": 0,
                        "total_tokens": 121
                    }),
                ),
            ],
        );

        let rows = load_source_view("blocks", false).unwrap();
        assert_eq!(rows.len(), 2);
        let total: i64 = rows
            .iter()
            .map(|row| row["totalTokens"].as_i64().unwrap_or(0))
            .sum();
        assert_eq!(total, 55 + 66);
    }

    #[test]
    fn non_forked_sessions_are_unaffected_by_burst_detector() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        let a = usage(50, 5);
        let b = usage(60, 6);

        fixture.write_session(
            "rollout-2026-06-09T20-46-23-main.jsonl",
            &[
                json!({
                    "timestamp": "2026-06-09T20:46:23.000Z",
                    "type": "session_meta",
                    "payload": { "model": "gpt-5.5" }
                }),
                turn_context("gpt-5.5", "2026-06-09"),
                token_count("2026-06-09T20:46:23.100Z", &a, &a),
                token_count(
                    "2026-06-09T20:46:23.101Z",
                    &b,
                    &json!({
                        "input_tokens": 110,
                        "cached_input_tokens": 0,
                        "output_tokens": 11,
                        "reasoning_output_tokens": 0,
                        "total_tokens": 121
                    }),
                ),
            ],
        );

        let rows = load_source_view("blocks", false).unwrap();
        assert_eq!(rows.len(), 2, "non-forked dense rows must all be counted");
    }

    fn usage(input: i64, output: i64) -> Value {
        json!({
            "input_tokens": input,
            "cached_input_tokens": 0,
            "output_tokens": output,
            "reasoning_output_tokens": 0,
            "total_tokens": input + output
        })
    }

    fn cum(parts: &[&Value]) -> Value {
        let mut input = 0i64;
        let mut output = 0i64;
        let mut total = 0i64;
        for part in parts {
            input += part["input_tokens"].as_i64().unwrap_or(0);
            output += part["output_tokens"].as_i64().unwrap_or(0);
            total += part["total_tokens"].as_i64().unwrap_or(0);
        }
        json!({
            "input_tokens": input,
            "cached_input_tokens": 0,
            "output_tokens": output,
            "reasoning_output_tokens": 0,
            "total_tokens": total
        })
    }

    fn session_meta_forked(model: &str) -> Value {
        json!({
            "timestamp": "2026-06-09T20:46:23.000Z",
            "type": "session_meta",
            "payload": {
                "forked_from_id": "019e095c-c041-7b40-b7cb-43ddb153086c",
                "model": model
            }
        })
    }

    fn turn_context(model: &str, current_date: &str) -> Value {
        json!({
            "timestamp": "2026-06-09T20:46:23.000Z",
            "type": "turn_context",
            "payload": {
                "model": model,
                "current_date": current_date
            }
        })
    }

    fn token_count(ts: &str, last: &Value, total: &Value) -> Value {
        json!({
            "timestamp": ts,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": last,
                    "total_token_usage": total
                }
            }
        })
    }

    struct TestCodexHome {
        root: PathBuf,
        previous_codex_home: Option<std::ffi::OsString>,
        previous_codex_homes: Option<std::ffi::OsString>,
        restores_env: bool,
    }

    impl TestCodexHome {
        fn new() -> Self {
            let fixture = Self::new_without_env("token-usage-codex");
            std::env::set_var("CODEX_HOME", &fixture.root);
            fixture
        }

        fn new_without_env(prefix: &str) -> Self {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("{prefix}-{now}"));
            fs::create_dir_all(root.join("sessions")).unwrap();
            fs::create_dir_all(root.join("archived_sessions")).unwrap();
            let previous_codex_home = std::env::var_os("CODEX_HOME");
            let previous_codex_homes = std::env::var_os(CODEX_HOMES_ENV);
            Self {
                root,
                previous_codex_home,
                previous_codex_homes,
                restores_env: prefix == "token-usage-codex",
            }
        }

        fn write_session(&self, relative_path: &str, lines: &[Value]) {
            let path = self.root.join("sessions").join(relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let contents = lines
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(path, contents).unwrap();
        }

        fn write_archived_session(&self, relative_path: &str, lines: &[Value]) {
            let path = self.root.join("archived_sessions").join(relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let contents = lines
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(path, contents).unwrap();
        }
    }

    impl Drop for TestCodexHome {
        fn drop(&mut self) {
            if self.restores_env {
                restore_env("CODEX_HOME", self.previous_codex_home.clone());
                restore_env(CODEX_HOMES_ENV, self.previous_codex_homes.clone());
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
