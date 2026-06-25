pub mod claude;
pub mod codex;
pub mod cursor;
pub mod cursor_api;
pub mod cursorpp;
pub mod grok;
pub mod hermes;
pub mod openclaw;
pub mod opencode;
pub mod pi;

use chrono::{DateTime, Local, TimeZone};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSource {
    Claude,
    Codex,
    Opencode,
    Hermes,
    OpenClaw,
    Pi,
    Grok,
    Cursor,
}

impl TryFrom<&str> for LocalSource {
    type Error = SourceError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "opencode" => Ok(Self::Opencode),
            "hermes" => Ok(Self::Hermes),
            "openclaw" => Ok(Self::OpenClaw),
            "pi" | "oh-my-pi" | "ohmypi" | "omp" => Ok(Self::Pi),
            "grok" => Ok(Self::Grok),
            "cursor" | "cursorpp" => Ok(Self::Cursor),
            other => Err(SourceError::UnsupportedSource(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceView {
    Daily,
    Monthly,
    Sessions,
    Blocks,
}

impl TryFrom<&str> for SourceView {
    type Error = SourceError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "daily" => Ok(Self::Daily),
            "monthly" => Ok(Self::Monthly),
            "sessions" => Ok(Self::Sessions),
            "blocks" => Ok(Self::Blocks),
            other => Err(SourceError::UnsupportedView(other.to_string())),
        }
    }
}

#[derive(Debug)]
pub enum SourceError {
    UnsupportedSource(String),
    UnsupportedView(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Source(String),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSource(source) => write!(f, "unsupported source: {source}"),
            Self::UnsupportedView(view) => write!(f, "unsupported view: {view}"),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Json(err) => write!(f, "json error: {err}"),
            Self::Source(err) => f.write_str(err),
            Self::Sqlite(err) => write!(f, "sqlite error: {err}"),
        }
    }
}

impl std::error::Error for SourceError {}

impl From<std::io::Error> for SourceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for SourceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<rusqlite::Error> for SourceError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

#[derive(Debug, Clone)]
pub struct LocalSession {
    pub session_id: String,
    pub date: String,
    pub time: String,
    pub model_name: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens_override: Option<i64>,
    pub total_cost: f64,
}

impl LocalSession {
    pub fn total_tokens(&self) -> i64 {
        self.total_tokens_override.unwrap_or_else(|| {
            self.input_tokens
                + self.output_tokens
                + self.cache_creation_tokens
                + self.cache_read_tokens
        })
    }
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
    model_breakdowns: Vec<Value>,
}

pub fn load_source_view(
    source: &str,
    view: &str,
    refresh: bool,
) -> Result<Vec<Value>, SourceError> {
    let source = LocalSource::try_from(source)?;
    let view = SourceView::try_from(view)?;
    load_local_source(source, view, refresh)
}

pub fn load_local_source(
    source: LocalSource,
    view: SourceView,
    _refresh: bool,
) -> Result<Vec<Value>, SourceError> {
    let view_name = match view {
        SourceView::Daily => "daily",
        SourceView::Monthly => "monthly",
        SourceView::Sessions => "sessions",
        SourceView::Blocks => "blocks",
    };

    match source {
        LocalSource::Claude => {
            return claude::load_source_view(view_name, _refresh).map_err(SourceError::Source);
        }
        LocalSource::Codex => {
            return codex::load_source_view(view_name, _refresh).map_err(SourceError::Source);
        }
        LocalSource::Opencode => {
            return opencode::load_source_view(view_name, _refresh).map_err(SourceError::Source);
        }
        LocalSource::Pi => {
            return pi::load_source_view(view_name, _refresh).map_err(SourceError::Source);
        }
        LocalSource::OpenClaw => {
            return openclaw::load_source_view(view_name, _refresh).map_err(SourceError::Source);
        }
        LocalSource::Hermes | LocalSource::Grok | LocalSource::Cursor => {}
    }

    if view == SourceView::Blocks {
        return Ok(Vec::new());
    }

    let sessions = match source {
        LocalSource::Claude
        | LocalSource::Codex
        | LocalSource::Opencode
        | LocalSource::OpenClaw
        | LocalSource::Pi
        => unreachable!(),
        LocalSource::Hermes => hermes::load_sessions()?,
        LocalSource::Grok => grok::load_sessions()?,
        LocalSource::Cursor => cursor::load_sessions()?,
    };

    Ok(match view {
        SourceView::Daily => sessions_to_daily(&sessions),
        SourceView::Monthly => sessions_to_monthly(&sessions),
        SourceView::Sessions => sessions_to_sessions(&sessions),
        SourceView::Blocks => Vec::new(),
    })
}

fn sessions_to_daily(sessions: &[LocalSession]) -> Vec<Value> {
    aggregate_sessions(sessions, |session| session.date.clone())
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
                "modelBreakdowns": group.model_breakdowns,
            })
        })
        .collect()
}

fn sessions_to_monthly(sessions: &[LocalSession]) -> Vec<Value> {
    aggregate_sessions(sessions, |session| session.date.chars().take(7).collect())
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
                "modelBreakdowns": group.model_breakdowns,
            })
        })
        .collect()
}

fn sessions_to_sessions(sessions: &[LocalSession]) -> Vec<Value> {
    let mut rows: Vec<_> = sessions
        .iter()
        .map(|session| {
            json!({
                "sessionId": session.session_id.clone(),
                "date": session.date.clone(),
                "time": session.time.clone(),
                "inputTokens": session.input_tokens,
                "outputTokens": session.output_tokens,
                "cacheCreationTokens": session.cache_creation_tokens,
                "cacheReadTokens": session.cache_read_tokens,
                "totalTokens": session.total_tokens(),
                "totalCost": session.total_cost,
                "modelsUsed": [cluster_model_name(&session.model_name)],
                "modelBreakdowns": [model_breakdown(session)],
            })
        })
        .collect();
    rows.sort_by_key(sort_key);
    rows
}

fn aggregate_sessions(
    sessions: &[LocalSession],
    key_for: impl Fn(&LocalSession) -> String,
) -> Vec<AggregateUsage> {
    let mut groups: BTreeMap<String, AggregateUsage> = BTreeMap::new();

    for session in sessions {
        let key = key_for(session);
        let group = groups.entry(key.clone()).or_insert_with(|| AggregateUsage {
            key,
            ..AggregateUsage::default()
        });

        group.input_tokens += session.input_tokens;
        group.output_tokens += session.output_tokens;
        group.cache_creation_tokens += session.cache_creation_tokens;
        group.cache_read_tokens += session.cache_read_tokens;
        group.total_tokens += session.total_tokens();
        group.total_cost += session.total_cost;

        let clustered = cluster_model_name(&session.model_name);
        if !group.models_used.contains(&clustered) {
            group.models_used.push(clustered);
        }
        group.model_breakdowns.push(model_breakdown(session));
    }

    groups.into_iter().map(|(_, group)| group).collect()
}

fn model_breakdown(session: &LocalSession) -> Value {
    json!({
        "modelName": cluster_model_name(&session.model_name),
        "inputTokens": session.input_tokens,
        "outputTokens": session.output_tokens,
        "cacheCreationTokens": session.cache_creation_tokens,
        "cacheReadTokens": session.cache_read_tokens,
        "cost": session.total_cost,
    })
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

/// Drops a leading provider prefix (`CPA/`, `cliproxy/`, `openrouter/foo/`, etc.).
fn strip_provider_prefix(model_name: &str) -> &str {
    model_name
        .rsplit('/')
        .next()
        .filter(|suffix| !suffix.is_empty())
        .unwrap_or(model_name)
}

/// `claude-{family}-4.8` and `claude-{family}-4-8` both become `claude-{family}-4-8`.
fn canonical_claude_4x_segment(segment: &str) -> Option<String> {
    let lower = segment.to_ascii_lowercase();
    let after_claude = lower.strip_prefix("claude-")?;
    let (family, version) = after_claude.split_once('-')?;
    if family.is_empty() {
        return None;
    }
    let (minor, suffix) = if let Some(rest) = version.strip_prefix("4.") {
        take_ascii_digits(rest)
    } else if let Some(rest) = version.strip_prefix("4-") {
        take_ascii_digits(rest)
    } else {
        return None;
    };
    if minor.is_empty() {
        return None;
    }
    Some(format!("claude-{family}-4-{minor}{suffix}"))
}

fn take_ascii_digits(s: &str) -> (String, &str) {
    let minor: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    let consumed = minor.len();
    (minor, &s[consumed..])
}

fn canonicalize_claude_4x_in_string(model_name: &str) -> String {
    let lower = model_name.to_ascii_lowercase();
    let Some(idx) = lower.find("claude-") else {
        return model_name.to_string();
    };
    let prefix = &model_name[..idx];
    let segment = &lower[idx..];
    let Some(canon) = canonical_claude_4x_segment(segment) else {
        return model_name.to_string();
    };
    format!("{prefix}{canon}")
}

/// Normalizes a model name by matching against known canonical model names.
/// Variants like `custom:deepseek-v4-flash` or `dqweqwe:deepseek-v4-pro21312421h43jk`
/// are merged into the canonical base model name.
pub(crate) fn cluster_model_name(model_name: &str) -> String {
    let lowered = model_name.trim().to_ascii_lowercase();
    if lowered == "zai-org/glm-5.2" || lowered == "zai-org/glm-5-2" {
        return "glm-5.2".to_string();
    }
    let stripped = strip_provider_prefix(&lowered);
    let mapped = if stripped.contains("ark-code-latest") || stripped == "ark-code" {
        "glm-5.2"
    } else if stripped.contains("glm-5.2") || stripped.contains("glm-5-2") {
        "glm-5.2"
    } else if stripped.contains("deepseek-v4-pro") {
        "deepseek-v4-pro"
    } else if stripped.contains("deepseek-v4-flash") {
        "deepseek-v4-flash"
    } else if stripped.contains("kimi-k2.6") {
        "kimi-k2.6"
    } else if stripped.contains("qwen3.6-plus") {
        "qwen3.6-plus"
    } else if stripped.contains("step-3.7-flash") {
        "step-3.7-flash"
    } else if stripped.contains("composer-2.5-fast") {
        "composer-2.5-fast"
    } else {
        stripped
    };
    canonicalize_claude_4x_in_string(mapped)
}

#[cfg(test)]
mod tests {
    use super::{cluster_model_name, LocalSource};
    use std::convert::TryFrom;

    #[test]
    fn cluster_model_name_maps_known_substrings() {
        assert_eq!(
            cluster_model_name("custom:deepseek-v4-flash"),
            "deepseek-v4-flash"
        );
        assert_eq!(cluster_model_name("3e12312kimi-k2.6213123"), "kimi-k2.6");
        assert_eq!(cluster_model_name("foo/qwen3.6-plus-bar"), "qwen3.6-plus");
        assert_eq!(cluster_model_name("zai-org/glm-5.2"), "glm-5.2");
        assert_eq!(
            cluster_model_name("openrouter/stepfun/step-3.7-flash-preview"),
            "step-3.7-flash"
        );
        assert_eq!(
            cluster_model_name("grok-composer-2.5-fast-extra"),
            "composer-2.5-fast"
        );
        assert_eq!(cluster_model_name("CPA/ark-code-latest"), "glm-5.2");
        assert_eq!(cluster_model_name("ark-code-latest"), "glm-5.2");
        assert_eq!(cluster_model_name("glm-5.2-max"), "glm-5.2");
        assert_eq!(cluster_model_name("z-ai/glm-5.2-preview"), "glm-5.2");
        assert_eq!(cluster_model_name("glm-5-2-max"), "glm-5.2");
        assert_eq!(cluster_model_name("cliproxy/gpt-5.5"), "gpt-5.5");
        assert_eq!(cluster_model_name("other-model"), "other-model");
        assert_eq!(cluster_model_name("GPT-5.5"), "gpt-5.5");
        assert_eq!(cluster_model_name("OTHER-MODEL"), "other-model");
        assert_eq!(cluster_model_name("Grok-Composer-2.5-Fast"), "composer-2.5-fast");
    }

    #[test]
    fn cluster_model_name_canonicalizes_claude_4x_versions() {
        assert_eq!(cluster_model_name("claude-opus-4.8"), "claude-opus-4-8");
        assert_eq!(cluster_model_name("claude-opus-4-8"), "claude-opus-4-8");
        assert_eq!(cluster_model_name("claude-opus-4.7"), "claude-opus-4-7");
        assert_eq!(cluster_model_name("anthropic/claude-opus-4.8"), "claude-opus-4-8");
        assert_eq!(cluster_model_name("claude-sonnet-4.5"), "claude-sonnet-4-5");
        assert_eq!(cluster_model_name("kiro-claude-opus-4.7"), "kiro-claude-opus-4-7");
    }

    #[test]
    fn oh_my_pi_aliases_map_to_pi() {
        assert_eq!(LocalSource::try_from("oh-my-pi").unwrap(), LocalSource::Pi);
        assert_eq!(LocalSource::try_from("ohmypi").unwrap(), LocalSource::Pi);
        assert_eq!(LocalSource::try_from("omp").unwrap(), LocalSource::Pi);
    }
}

pub(crate) fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

pub(crate) fn unix_seconds_to_utc_parts(seconds: f64) -> Option<(String, String)> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }

    let whole_seconds = seconds.trunc() as i64;
    let nanos = ((seconds.fract().abs()) * 1_000_000_000.0).round() as u32;
    let nanos = nanos.min(999_999_999);
    let date_time = DateTime::from_timestamp(whole_seconds, nanos)?;
    Some(local_parts(date_time.with_timezone(&Local)))
}

pub(crate) fn unix_millis_to_utc_parts(milliseconds: i64) -> Option<(String, String)> {
    if milliseconds <= 0 {
        return None;
    }

    Local
        .timestamp_millis_opt(milliseconds)
        .single()
        .map(local_parts)
}

pub(crate) fn to_i64(value: &Value) -> i64 {
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

pub(crate) fn num(value: &Value) -> f64 {
    if let Some(number) = value.as_f64() {
        return if number.is_finite() { number } else { 0.0 };
    }
    value
        .as_str()
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|number| number.is_finite())
        .unwrap_or_default()
}

fn local_parts(date_time: DateTime<Local>) -> (String, String) {
    (
        date_time.format("%Y-%m-%d").to_string(),
        date_time.format("%H:%M").to_string(),
    )
}
