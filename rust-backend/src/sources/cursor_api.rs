use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, LocalSession, SourceError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Datelike, NaiveDate, Utc};
use reqwest::blocking::Client;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const CURSOR_STATE_DB_ENV: &str = "CURSOR_STATE_DB_PATH";
const CURSOR_API_BASE: &str = "https://cursor.com";
const REQUEST_TIMEOUT_SECS: u64 = 30;
const USAGE_EVENTS_PAGE_SIZE: i64 = 100;
const USAGE_EVENTS_MAX_PAGES: i64 = 50;

pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let Some((user_id, session_token)) = load_session_auth()? else {
        return Ok(Vec::new());
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|err| SourceError::Source(err.to_string()))?;

    let mut sessions = Vec::new();

    if let Ok(summary) = fetch_usage_summary(&client, &session_token) {
        if let Ok(events) = fetch_filtered_usage_events(
            &client,
            &session_token,
            summary.billing_cycle_start_ms,
            summary.billing_cycle_end_ms,
        ) {
            sessions.extend(usage_events_to_sessions(&events));
        }
    }

    let usage = fetch_usage(&client, &user_id, &session_token)?;
    sessions.extend(usage_to_sessions(&usage));

    let periods = invoice_periods(&usage.start_of_month);
    for (year, month) in periods {
        if let Ok(invoice) = fetch_monthly_invoice(&client, &session_token, year, month) {
            sessions.extend(invoice_to_sessions(year, month, &invoice));
        }
    }

    Ok(sessions)
}

#[derive(Debug, Clone)]
struct UsageSnapshot {
    start_of_month: String,
    buckets: Vec<UsageBucket>,
}

#[derive(Debug, Clone)]
struct UsageBucket {
    name: String,
    num_requests: i64,
    num_tokens: i64,
}

#[derive(Debug, Clone)]
struct InvoiceItem {
    description: String,
    cents: f64,
}

#[derive(Debug, Clone)]
struct UsageSummaryWindow {
    billing_cycle_start_ms: i64,
    billing_cycle_end_ms: i64,
}

#[derive(Debug, Clone)]
struct UsageEventRecord {
    timestamp_ms: i64,
    model_name: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
}

fn load_session_auth() -> Result<Option<(String, String)>, SourceError> {
    let db_path = discover_state_db_path();
    let Some(db_path) = db_path else {
        return Ok(None);
    };
    if !db_path.is_file() {
        return Ok(None);
    }

    let access_token = read_access_token(&db_path)?;
    let Some(access_token) = access_token else {
        return Ok(None);
    };

    let user_id = user_id_from_access_token(&access_token)?;
    let session_token = format!("{user_id}%3A%3A{access_token}");
    Ok(Some((user_id, session_token)))
}

fn discover_state_db_path() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(CURSOR_STATE_DB_ENV) {
        return Some(PathBuf::from(raw));
    }
    home_dir().map(|home| {
        home.join("Library")
            .join("Application Support")
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb")
    })
}

fn read_access_token(db_path: &Path) -> Result<Option<String>, SourceError> {
    let connection = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let token = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(token.filter(|value| !value.trim().is_empty()))
}

fn user_id_from_access_token(access_token: &str) -> Result<String, SourceError> {
    let payload = jwt_payload(access_token)?;
    let sub = payload
        .get("sub")
        .and_then(Value::as_str)
        .ok_or_else(|| SourceError::Source("cursor access token missing sub".to_string()))?;
    let user_id = sub
        .split('|')
        .nth(1)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SourceError::Source("cursor access token sub is malformed".to_string()))?;
    Ok(user_id.to_string())
}

fn jwt_payload(access_token: &str) -> Result<Value, SourceError> {
    let encoded = access_token
        .split('.')
        .nth(1)
        .ok_or_else(|| SourceError::Source("cursor access token is not a JWT".to_string()))?;
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|err| SourceError::Source(format!("cursor access token payload decode failed: {err}")))?;
    serde_json::from_slice(&decoded)
        .map_err(|err| SourceError::Source(format!("cursor access token payload json failed: {err}")))
}

fn browser_headers(session_token: &str) -> Vec<(String, String)> {
    vec![
        ("Cookie".to_string(), format!("WorkosCursorSessionToken={session_token}")),
        ("Origin".to_string(), "https://cursor.com".to_string()),
        ("Referer".to_string(), "https://cursor.com/dashboard".to_string()),
        ("Accept".to_string(), "application/json".to_string()),
        (
            "User-Agent".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36".to_string(),
        ),
    ]
}

fn fetch_usage(
    client: &Client,
    user_id: &str,
    session_token: &str,
) -> Result<UsageSnapshot, SourceError> {
    let mut request = client.get(format!("{CURSOR_API_BASE}/api/usage")).query(&[("user", user_id)]);
    for (key, value) in browser_headers(session_token) {
        request = request.header(key, value);
    }

    let response = request
        .send()
        .map_err(|err| SourceError::Source(format!("cursor usage request failed: {err}")))?;
    if !response.status().is_success() {
        return Err(SourceError::Source(format!(
            "cursor usage request returned {}",
            response.status()
        )));
    }

    let payload: Value = response
        .json()
        .map_err(|err| SourceError::Source(format!("cursor usage response json failed: {err}")))?;

    let start_of_month = payload
        .get("startOfMonth")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let mut buckets = Vec::new();
    if let Some(object) = payload.as_object() {
        for (name, value) in object {
            if name == "startOfMonth" {
                continue;
            }
            let Some(bucket) = value.as_object() else {
                continue;
            };
            buckets.push(UsageBucket {
                name: name.clone(),
                num_requests: bucket
                    .get("numRequests")
                    .map(crate::sources::to_i64)
                    .unwrap_or_default(),
                num_tokens: bucket
                    .get("numTokens")
                    .map(crate::sources::to_i64)
                    .unwrap_or_default(),
            });
        }
    }

    Ok(UsageSnapshot {
        start_of_month,
        buckets,
    })
}

fn fetch_usage_summary(
    client: &Client,
    session_token: &str,
) -> Result<UsageSummaryWindow, SourceError> {
    let mut request = client.get(format!("{CURSOR_API_BASE}/api/usage-summary"));
    for (key, value) in browser_headers(session_token) {
        request = request.header(key, value);
    }

    let response = request
        .send()
        .map_err(|err| SourceError::Source(format!("cursor usage-summary request failed: {err}")))?;
    if !response.status().is_success() {
        return Err(SourceError::Source(format!(
            "cursor usage-summary request returned {}",
            response.status()
        )));
    }

    let payload: Value = response.json().map_err(|err| {
        SourceError::Source(format!("cursor usage-summary response json failed: {err}"))
    })?;

    let start = parse_iso_to_epoch_millis(
        payload
            .get("billingCycleStart")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let end = parse_iso_to_epoch_millis(
        payload
            .get("billingCycleEnd")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if start <= 0 || end <= 0 || end < start {
        return Err(SourceError::Source(
            "cursor usage-summary missing billing cycle bounds".to_string(),
        ));
    }

    Ok(UsageSummaryWindow {
        billing_cycle_start_ms: start,
        billing_cycle_end_ms: end,
    })
}

fn fetch_filtered_usage_events(
    client: &Client,
    session_token: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<UsageEventRecord>, SourceError> {
    let mut events = Vec::new();
    for page in 1..=USAGE_EVENTS_MAX_PAGES {
        let body = serde_json::json!({
            "startDate": start_ms.to_string(),
            "endDate": end_ms.to_string(),
            "page": page,
            "pageSize": USAGE_EVENTS_PAGE_SIZE,
        });

        let mut request = client
            .post(format!("{CURSOR_API_BASE}/api/dashboard/get-filtered-usage-events"))
            .json(&body);
        for (key, value) in browser_headers(session_token) {
            request = request.header(key, value);
        }
        request = request.header("Content-Type", "application/json");

        let response = request.send().map_err(|err| {
            SourceError::Source(format!("cursor usage-events request failed: {err}"))
        })?;
        if !response.status().is_success() {
            return Err(SourceError::Source(format!(
                "cursor usage-events request returned {}",
                response.status()
            )));
        }

        let payload: Value = response.json().map_err(|err| {
            SourceError::Source(format!("cursor usage-events response json failed: {err}"))
        })?;

        let page_events = parse_usage_events_payload(&payload);
        let page_len = page_events.len() as i64;
        events.extend(page_events);

        if page_len < USAGE_EVENTS_PAGE_SIZE {
            break;
        }
    }

    Ok(events)
}

fn parse_usage_events_payload(payload: &Value) -> Vec<UsageEventRecord> {
    let rows = payload
        .get("usageEventsDisplay")
        .or_else(|| payload.get("usageEvents"))
        .and_then(Value::as_array);

    let Some(rows) = rows else {
        return Vec::new();
    };

    rows.iter().filter_map(parse_usage_event_row).collect()
}

fn parse_usage_event_row(row: &Value) -> Option<UsageEventRecord> {
    let timestamp_ms = row
        .get("timestamp")
        .map(crate::sources::to_i64)
        .filter(|value| *value > 0)?;
    let model_name = row
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)?;

    let token_usage = row.get("tokenUsage").unwrap_or(row);
    let input_tokens = token_field(token_usage, &["inputTokens", "input_tokens"]);
    let output_tokens = token_field(token_usage, &["outputTokens", "output_tokens"]);
    let cache_creation_tokens = token_field(
        token_usage,
        &["cacheWriteTokens", "cacheCreationTokens", "cache_write_tokens"],
    );
    let cache_read_tokens =
        token_field(token_usage, &["cacheReadTokens", "cache_read_tokens"]);

    if input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens <= 0 {
        return None;
    }

    Some(UsageEventRecord {
        timestamp_ms,
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    })
}

fn usage_events_to_sessions(events: &[UsageEventRecord]) -> Vec<LocalSession> {
    events
        .iter()
        .map(|event| {
            let (date, time) = epoch_millis_to_date_time(event.timestamp_ms);
            let model_name = event.model_name.clone();
            let usage = TokenUsage {
                input_tokens: event.input_tokens,
                output_tokens: event.output_tokens,
                cache_creation_tokens: event.cache_creation_tokens,
                cache_read_tokens: event.cache_read_tokens,
            };
            LocalSession {
                session_id: format!("cursor:api:event:{}", event.timestamp_ms),
                date,
                time,
                model_name: model_name.clone(),
                input_tokens: event.input_tokens,
                output_tokens: event.output_tokens,
                cache_creation_tokens: event.cache_creation_tokens,
                cache_read_tokens: event.cache_read_tokens,
                total_tokens_override: None,
                total_cost: model_cost_usd(&model_name, usage),
            }
        })
        .collect()
}

fn token_field(value: &Value, keys: &[&str]) -> i64 {
    for key in keys {
        if let Some(field) = value.get(*key) {
            return crate::sources::to_i64(field);
        }
    }
    0
}

fn parse_iso_to_epoch_millis(raw: &str) -> i64 {
    if raw.is_empty() {
        return 0;
    }
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
        return parsed.timestamp_millis();
    }
    crate::sources::to_i64(&Value::String(raw.to_string()))
}

fn epoch_millis_to_date_time(milliseconds: i64) -> (String, String) {
    crate::sources::unix_millis_to_utc_parts(milliseconds).unwrap_or_else(|| {
        let now = Utc::now();
        (now.format("%Y-%m-%d").to_string(), now.format("%H:%M").to_string())
    })
}

fn fetch_monthly_invoice(
    client: &Client,
    session_token: &str,
    year: i32,
    month: u32,
) -> Result<Vec<InvoiceItem>, SourceError> {
    let body = serde_json::json!({
        "month": month,
        "year": year,
        "includeUsageEvents": false,
    });

    let mut request = client
        .post(format!("{CURSOR_API_BASE}/api/dashboard/get-monthly-invoice"))
        .json(&body);
    for (key, value) in browser_headers(session_token) {
        request = request.header(key, value);
    }
    request = request.header("Content-Type", "application/json");

    let response = request
        .send()
        .map_err(|err| SourceError::Source(format!("cursor invoice request failed: {err}")))?;
    if !response.status().is_success() {
        return Err(SourceError::Source(format!(
            "cursor invoice request returned {}",
            response.status()
        )));
    }

    let payload: Value = response
        .json()
        .map_err(|err| SourceError::Source(format!("cursor invoice response json failed: {err}")))?;

    let mut items = Vec::new();
    if let Some(rows) = payload.get("items").and_then(Value::as_array) {
        for row in rows {
            let Some(description) = row.get("description").and_then(Value::as_str) else {
                continue;
            };
            if description.contains("Mid-month usage paid") {
                continue;
            }
            let Some(cents) = row.get("cents").and_then(value_to_f64) else {
                continue;
            };
            items.push(InvoiceItem {
                description: description.to_string(),
                cents,
            });
        }
    }
    Ok(items)
}

fn usage_to_sessions(usage: &UsageSnapshot) -> Vec<LocalSession> {
    let (date, time) = iso_to_date_time(&usage.start_of_month);
    let period_key = date.replace('-', "");

    usage
        .buckets
        .iter()
        .filter(|bucket| bucket.num_requests > 0 || bucket.num_tokens > 0)
        .map(|bucket| {
            let model_name = usage_bucket_model_name(&bucket.name);
            let total_tokens = if bucket.num_tokens > 0 {
                bucket.num_tokens
            } else {
                bucket.num_requests
            };
            let usage = TokenUsage {
                input_tokens: total_tokens,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            };
            LocalSession {
                session_id: format!("cursor:api:usage:{}:{period_key}", bucket.name),
                date: date.clone(),
                time: time.clone(),
                model_name: model_name.clone(),
                input_tokens: total_tokens,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                total_tokens_override: Some(total_tokens),
                total_cost: model_cost_usd(&model_name, usage),
            }
        })
        .collect()
}

fn invoice_to_sessions(year: i32, month: u32, items: &[InvoiceItem]) -> Vec<LocalSession> {
    let date = month_anchor_date(year, month);
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let (request_count, model_name) = parse_invoice_description(&item.description)?;
            if request_count <= 0 {
                return None;
            }
            let usage = TokenUsage {
                input_tokens: request_count,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            };
            Some(LocalSession {
                session_id: format!("cursor:api:invoice:{year:04}-{month:02}:{index}"),
                date: date.clone(),
                time: "23:59".to_string(),
                model_name: model_name.clone(),
                input_tokens: request_count,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                total_tokens_override: Some(request_count),
                total_cost: model_cost_usd(&model_name, usage),
            })
        })
        .collect()
}

fn usage_bucket_model_name(bucket: &str) -> String {
    match bucket {
        "gpt-4" => "cursor-fast-requests".to_string(),
        "gpt-4-32k" => "cursor-usage-based".to_string(),
        "gpt-3.5-turbo" => "cursor-legacy".to_string(),
        other => format!("cursor-{other}"),
    }
}

fn parse_invoice_description(description: &str) -> Option<(i64, String)> {
    if let Some(captures) = regex_token_based(description) {
        return Some((captures.0, captures.1));
    }

    let count = description
        .split_whitespace()
        .next()?
        .parse::<i64>()
        .ok()?;
    let model = if description.contains("tool calls") {
        "cursor-tool-calls".to_string()
    } else if let Some(model) = regex_model_name(description) {
        format!("cursor-{model}")
    } else if description.contains("extra fast premium request") {
        "cursor-fast-premium".to_string()
    } else {
        "cursor-billed-usage".to_string()
    };
    Some((count, model))
}

fn regex_token_based(description: &str) -> Option<(i64, String)> {
    let mut parts = description.splitn(2, ' ');
    let count = parts.next()?.parse::<i64>().ok()?;
    let tail = parts.next()?;
    if !tail.starts_with("token-based usage calls to ") {
        return None;
    }
    let model = tail
        .strip_prefix("token-based usage calls to ")?
        .split(',')
        .next()?
        .trim()
        .to_string();
    Some((count, format!("cursor-{model}")))
}

fn regex_model_name(description: &str) -> Option<String> {
    for token in [
        "claude-4-sonnet-thinking",
        "claude-4.5-sonnet",
        "claude-4-sonnet",
        "claude-3.7-sonnet",
        "claude-3.5-sonnet",
        "claude-3-opus",
        "claude-3-sonnet",
        "claude-3-haiku",
        "gpt-4o-128k",
        "gpt-4o",
        "gpt-4.1",
        "gpt-4",
        "gpt-3.5-turbo",
        "gemini-2.5-pro",
        "gemini-1.5-flash",
        "o3-mini",
        "o1-mini",
        "o1",
    ] {
        if description.contains(token) {
            return Some(token.to_string());
        }
    }
    None
}

fn invoice_periods(start_of_month: &str) -> Vec<(i32, u32)> {
    let current = iso_to_naive_date(start_of_month).unwrap_or_else(|| Utc::now().date_naive());
    let previous = previous_month(current);
    vec![
        (current.year(), current.month()),
        (previous.year(), previous.month()),
    ]
}

fn previous_month(date: NaiveDate) -> NaiveDate {
    if date.month() == 1 {
        NaiveDate::from_ymd_opt(date.year() - 1, 12, 1).unwrap_or(date)
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() - 1, 1).unwrap_or(date)
    }
}

fn month_anchor_date(year: i32, month: u32) -> String {
    let last_day = last_day_of_month(year, month);
    format!("{year:04}-{month:02}-{last_day:02}")
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next =
        NaiveDate::from_ymd_opt(next_year, next_month, 1).unwrap_or_else(|| Utc::now().date_naive());
    first_of_next.pred_opt().map(|date| date.day()).unwrap_or(28)
}

fn iso_to_date_time(raw: &str) -> (String, String) {
    if let Some(date) = iso_to_naive_date(raw) {
        return (date.format("%Y-%m-%d").to_string(), "00:00".to_string());
    }
    (Utc::now().format("%Y-%m-%d").to_string(), "00:00".to_string())
}

fn iso_to_naive_date(raw: &str) -> Option<NaiveDate> {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.date_naive());
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()
}

fn value_to_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .or_else(|| value.as_i64().map(|number| number as f64))
        .or_else(|| value.as_u64().map(|number| number as f64))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.parse::<f64>().ok())
                .filter(|number| number.is_finite())
        })
}

#[cfg(test)]
mod tests {
    use super::{
        invoice_to_sessions, parse_invoice_description, parse_usage_event_row,
        usage_bucket_model_name, usage_events_to_sessions, usage_to_sessions, InvoiceItem,
        UsageBucket, UsageEventRecord, UsageSnapshot,
    };

    #[test]
    fn maps_usage_buckets_to_cursor_api_sessions() {
        let sessions = usage_to_sessions(&UsageSnapshot {
            start_of_month: "2026-06-25T11:51:45.000Z".to_string(),
            buckets: vec![UsageBucket {
                name: "gpt-4".to_string(),
                num_requests: 12,
                num_tokens: 3456,
            }],
        });
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "cursor:api:usage:gpt-4:20260625");
        assert_eq!(sessions[0].model_name, "cursor-fast-requests");
        assert_eq!(sessions[0].total_tokens(), 3456);
        assert_eq!(sessions[0].total_cost, 0.0);
    }

    #[test]
    fn parses_invoice_descriptions_into_billing_sessions() {
        let items = vec![
            InvoiceItem {
                description: "18 token-based usage calls to claude-4.5-sonnet, totalling: $4.20"
                    .to_string(),
                cents: 420.0,
            },
            InvoiceItem {
                description: "Mid-month usage paid".to_string(),
                cents: -100.0,
            },
        ];
        let sessions = invoice_to_sessions(2026, 6, &items);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "cursor:api:invoice:2026-06:0");
        assert_eq!(sessions[0].model_name, "cursor-claude-4.5-sonnet");
        assert_eq!(sessions[0].input_tokens, 18);
        assert!(sessions[0].total_cost >= 0.0);
    }

    #[test]
    fn parse_invoice_description_handles_fast_premium_lines() {
        let parsed = parse_invoice_description("7 extra fast premium requests (Haiku) beyond")
            .unwrap();
        assert_eq!(parsed.0, 7);
        assert_eq!(parsed.1, "cursor-fast-premium");
    }

    #[test]
    fn usage_bucket_model_names_are_stable() {
        assert_eq!(usage_bucket_model_name("gpt-4"), "cursor-fast-requests");
        assert_eq!(usage_bucket_model_name("gpt-4-32k"), "cursor-usage-based");
    }

    #[test]
    fn parses_filtered_usage_event_rows_into_sessions() {
        let row = serde_json::json!({
            "timestamp": "1782871200000",
            "model": "glm-5.2-max",
            "kind": "USAGE_EVENT_KIND_INCLUDED_IN_PRO",
            "tokenUsage": {
                "inputTokens": 100,
                "outputTokens": 200,
                "cacheWriteTokens": 300,
                "cacheReadTokens": 400,
                "totalCents": 0
            },
            "chargedCents": 0
        });
        let event = parse_usage_event_row(&row).unwrap();
        assert_eq!(event.model_name, "glm-5.2-max");
        assert_eq!(event.input_tokens, 100);
        assert_eq!(event.output_tokens, 200);
        assert_eq!(event.cache_creation_tokens, 300);
        assert_eq!(event.cache_read_tokens, 400);

        let sessions = usage_events_to_sessions(&[event]);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "cursor:api:event:1782871200000");
        assert_eq!(sessions[0].total_tokens(), 1000);
    }
}