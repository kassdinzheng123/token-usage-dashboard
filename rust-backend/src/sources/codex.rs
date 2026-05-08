use crate::pricing::{model_cost_usd, TokenUsage};
use chrono::{DateTime, Local};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceView {
    Daily,
    Monthly,
    Sessions,
    Blocks,
}

impl TryFrom<&str> for SourceView {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "daily" => Ok(Self::Daily),
            "monthly" => Ok(Self::Monthly),
            "sessions" => Ok(Self::Sessions),
            "blocks" => Ok(Self::Blocks),
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

pub fn load_source_view(view: &str, refresh: bool) -> Result<Vec<Value>, String> {
    let _ = refresh;
    let view = SourceView::try_from(view)?;

    if view == SourceView::Blocks {
        return Ok(Vec::new());
    }

    let events = load_events()?;
    Ok(match view {
        SourceView::Daily => events_to_daily(&events),
        SourceView::Monthly => events_to_monthly(&events),
        SourceView::Sessions => events_to_sessions(&events),
        SourceView::Blocks => Vec::new(),
    })
}

pub fn load_daily_for_date(date: &str, refresh: bool) -> Result<Vec<Value>, String> {
    let _ = refresh;
    let events = load_events_for_date(date)?;
    Ok(events_to_daily(&events)
        .into_iter()
        .filter(|row| row.get("date").and_then(Value::as_str) == Some(date))
        .collect())
}

fn load_events() -> Result<Vec<TokenUsageEvent>, String> {
    let Some(sessions_dir) = codex_home().map(|home| home.join("sessions")) else {
        return Ok(Vec::new());
    };

    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_jsonl_files(&sessions_dir, &mut files)?;
    files.sort();

    let mut events = Vec::new();
    for file in files {
        append_events_from_file(&sessions_dir, &file, &mut events)?;
    }

    events.sort_by_key(|event| event.timestamp_millis);
    Ok(events)
}

fn load_events_for_date(date: &str) -> Result<Vec<TokenUsageEvent>, String> {
    let Some(sessions_dir) = codex_home().map(|home| home.join("sessions")) else {
        return Ok(Vec::new());
    };

    let Some((year, month, day)) = date_parts(date) else {
        return Ok(Vec::new());
    };

    let day_dir = sessions_dir.join(year).join(month).join(day);
    if !day_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_jsonl_files(&day_dir, &mut files)?;
    files.sort();

    let mut events = Vec::new();
    for file in files {
        append_events_from_file(&sessions_dir, &file, &mut events)?;
    }

    events.retain(|event| event.date == date);
    events.sort_by_key(|event| event.timestamp_millis);
    Ok(events)
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
    sessions_dir: &Path,
    file_path: &Path,
    events: &mut Vec<TokenUsageEvent>,
) -> Result<(), String> {
    let file = match File::open(file_path) {
        Ok(file) => file,
        Err(_) => return Ok(()),
    };

    let session_id = session_id_for(sessions_dir, file_path);
    let mut previous_totals: Option<RawUsage> = None;
    let mut current_model: Option<String> = None;

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

        if entry.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(model) = extract_model(payload) {
                current_model = Some(model);
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

        let raw_usage = match (last_usage, total_usage) {
            (Some(last), total) => {
                if let Some(total) = total {
                    previous_totals = Some(total);
                }
                Some(last)
            }
            (None, Some(total)) => {
                let delta = subtract_raw_usage(total, previous_totals);
                previous_totals = Some(total);
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

        let extracted_model = extract_model(payload);
        if let Some(model) = extracted_model.as_ref() {
            current_model = Some(model.clone());
        }
        let (model_name, is_fallback_model) = extracted_model
            .or_else(|| current_model.clone())
            .map(|model| (model, false))
            .unwrap_or_else(|| (UNKNOWN_MODEL.to_string(), true));
        let cache_read_tokens = raw_usage
            .cached_input_tokens
            .min(raw_usage.input_tokens)
            .max(0);
        let non_cached_input = raw_usage.input_tokens.saturating_sub(cache_read_tokens);
        let total_tokens = if raw_usage.total_tokens > 0 {
            raw_usage.total_tokens
        } else {
            raw_usage.input_tokens + raw_usage.output_tokens
        };

        let explicit_cost = explicit_cost(payload);
        let cost = if explicit_cost > 0.0 {
            explicit_cost
        } else {
            model_cost_usd(
                if is_fallback_model {
                    FALLBACK_PRICING_MODEL
                } else {
                    &model_name
                },
                TokenUsage {
                    input_tokens: non_cached_input,
                    output_tokens: raw_usage.output_tokens,
                    cache_creation_tokens: 0,
                    cache_read_tokens,
                },
            )
        };

        events.push(TokenUsageEvent {
            session_id: session_id.clone(),
            date,
            time,
            timestamp_millis,
            model_name,
            is_fallback_model,
            input_tokens: non_cached_input,
            output_tokens: raw_usage.output_tokens,
            cache_read_tokens,
            total_tokens,
            cost,
        });
    }

    Ok(())
}

fn events_to_daily(events: &[TokenUsageEvent]) -> Vec<Value> {
    aggregate_events(events, |event| event.date.clone())
        .into_iter()
        .map(|group| {
            json!({
                "date": group.key,
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

fn events_to_monthly(events: &[TokenUsageEvent]) -> Vec<Value> {
    aggregate_events(events, |event| event.date.chars().take(7).collect())
        .into_iter()
        .map(|group| {
            json!({
                "month": group.key,
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
            .entry(event.model_name.clone())
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

fn date_parts(date: &str) -> Option<(&str, &str, &str)> {
    let mut parts = date.split('-');
    let year = parts.next()?;
    let month = parts.next()?;
    let day = parts.next()?;
    if parts.next().is_some()
        || year.len() != 4
        || month.len() != 2
        || day.len() != 2
        || !date.chars().all(|ch| ch.is_ascii_digit() || ch == '-')
    {
        return None;
    }
    Some((year, month, day))
}

fn session_id_for(sessions_dir: &Path, file_path: &Path) -> String {
    let relative = file_path.strip_prefix(sessions_dir).unwrap_or(file_path);
    let without_extension = relative.with_extension("");
    without_extension
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
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
mod tests {
    use super::*;
    use std::{
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
    fn returns_empty_blocks_view() {
        let _guard = ENV_LOCK.lock().unwrap();
        let fixture = TestCodexHome::new();
        fixture.write_session("ignored.jsonl", &[]);

        let rows = load_source_view("blocks", true).unwrap();
        assert!(rows.is_empty());
    }

    struct TestCodexHome {
        root: PathBuf,
        previous_codex_home: Option<std::ffi::OsString>,
    }

    impl TestCodexHome {
        fn new() -> Self {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("token-usage-codex-{now}"));
            fs::create_dir_all(root.join("sessions")).unwrap();
            let previous_codex_home = std::env::var_os("CODEX_HOME");
            std::env::set_var("CODEX_HOME", &root);
            Self {
                root,
                previous_codex_home,
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
    }

    impl Drop for TestCodexHome {
        fn drop(&mut self) {
            if let Some(value) = &self.previous_codex_home {
                std::env::set_var("CODEX_HOME", value);
            } else {
                std::env::remove_var("CODEX_HOME");
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
