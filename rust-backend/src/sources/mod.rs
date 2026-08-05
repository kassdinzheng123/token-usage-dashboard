pub mod cherry;
pub mod claude;
pub mod claude_science;
pub mod codex;
pub mod cursor;
pub mod cursor_api;
pub mod cursorpp;
pub mod grok;
pub mod hermes;
pub mod kimi;
pub mod openclaw;
pub mod opencode;
pub mod pi;
pub mod reasonix;
pub mod zcode;

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
    Cherry,
    ClaudeScience,
    Zcode,
    Kimi,
    Reasonix,
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
            "cherry" | "cherrystudio" | "cherry-studio" => Ok(Self::Cherry),
            "claude-science" | "claude_science" | "claudescience" => Ok(Self::ClaudeScience),
            "zcode" | "z-code" | "z_code" => Ok(Self::Zcode),
            "kimi" | "kimi-code" | "kimi-work" | "kimicode" | "kimiwork" => Ok(Self::Kimi),
            "reasonix" | "reason-ix" => Ok(Self::Reasonix),
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
    Messages,
}

impl TryFrom<&str> for SourceView {
    type Error = SourceError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "daily" => Ok(Self::Daily),
            "monthly" => Ok(Self::Monthly),
            "sessions" => Ok(Self::Sessions),
            "blocks" => Ok(Self::Blocks),
            "messages" => Ok(Self::Messages),
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
pub struct LocalModelUsage {
    pub model_name: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cost: f64,
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
    pub model_breakdowns: Vec<LocalModelUsage>,
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
    load_source_view_since(source, view, refresh, None)
}

/// Incremental variant of [`load_source_view`]: file-walking adapters skip
/// files whose mtime is at or before `watermark_ms` (epoch millis of the
/// previous successful scan's start). Rows from skipped files stay in the
/// ledger from earlier ingests. `TOKEN_USAGE_FULL_RESCAN=1` forces a full
/// scan regardless of the watermark.
pub fn load_source_view_since(
    source: &str,
    view: &str,
    refresh: bool,
    watermark_ms: Option<i64>,
) -> Result<Vec<Value>, SourceError> {
    let source = LocalSource::try_from(source)?;
    let view = SourceView::try_from(view)?;
    load_local_source_since(source, view, refresh, watermark_ms)
}

pub fn load_local_source(
    source: LocalSource,
    view: SourceView,
    refresh: bool,
) -> Result<Vec<Value>, SourceError> {
    load_local_source_since(source, view, refresh, None)
}

pub fn load_local_source_since(
    source: LocalSource,
    view: SourceView,
    _refresh: bool,
    watermark_ms: Option<i64>,
) -> Result<Vec<Value>, SourceError> {
    let watermark_ms = if full_rescan_requested() {
        None
    } else {
        watermark_ms
    };

    let view_name = match view {
        SourceView::Daily => "daily",
        SourceView::Monthly => "monthly",
        SourceView::Sessions => "sessions",
        SourceView::Blocks => "blocks",
        SourceView::Messages => "messages",
    };

    match source {
        LocalSource::Claude => {
            return claude::load_source_view_since(view_name, _refresh, watermark_ms)
                .map_err(SourceError::Source);
        }
        LocalSource::Codex => {
            return codex::load_source_view_since(view_name, _refresh, watermark_ms)
                .map_err(SourceError::Source);
        }
        LocalSource::Opencode => {
            return opencode::load_source_view_since(view_name, _refresh, watermark_ms)
                .map_err(SourceError::Source);
        }
        LocalSource::Pi => {
            return pi::load_source_view_since(view_name, _refresh, watermark_ms)
                .map_err(SourceError::Source);
        }
        LocalSource::OpenClaw => {
            return openclaw::load_source_view_since(view_name, _refresh, watermark_ms)
                .map_err(SourceError::Source);
        }
        LocalSource::Kimi if view == SourceView::Messages => {
            return kimi::load_messages(watermark_ms);
        }
        LocalSource::Hermes
        | LocalSource::Grok
        | LocalSource::Cursor
        | LocalSource::Cherry
        | LocalSource::ClaudeScience
        | LocalSource::Zcode
        | LocalSource::Kimi
        | LocalSource::Reasonix => {}
    }

    // Message-level rows only exist for sources with per-message timestamps in
    // their local logs. Grok/Cherry/ZCode/Claude Science/Cursor session rows
    // are already event-level; Hermes has nothing finer than a session. All of
    // them keep session-level hourly attribution via the ledger fallback.
    if view == SourceView::Blocks || view == SourceView::Messages {
        return Ok(Vec::new());
    }

    let sessions = match source {
        LocalSource::Claude
        | LocalSource::Codex
        | LocalSource::Opencode
        | LocalSource::OpenClaw
        | LocalSource::Pi => unreachable!(),
        LocalSource::Hermes => hermes::load_sessions()?,
        LocalSource::Grok => grok::load_sessions(watermark_ms)?,
        LocalSource::Cursor => cursor::load_sessions(watermark_ms)?,
        LocalSource::Cherry => cherry::load_sessions()?,
        LocalSource::ClaudeScience => claude_science::load_sessions()?,
        LocalSource::Zcode => zcode::load_sessions()?,
        LocalSource::Kimi => kimi::load_sessions(watermark_ms)?,
        LocalSource::Reasonix => reasonix::load_sessions(watermark_ms)?,
    };

    Ok(match view {
        SourceView::Daily => sessions_to_daily(&sessions),
        SourceView::Monthly => sessions_to_monthly(&sessions),
        SourceView::Sessions => sessions_to_sessions(&sessions),
        SourceView::Blocks | SourceView::Messages => Vec::new(),
    })
}

/// `TOKEN_USAGE_FULL_RESCAN=1` (or true/yes/on) disables watermark filtering.
pub(crate) fn full_rescan_requested() -> bool {
    std::env::var("TOKEN_USAGE_FULL_RESCAN")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// True when `path`'s mtime is strictly newer than `watermark_ms` (epoch
/// millis). Metadata failures fail open so the file is re-read.
pub(crate) fn file_modified_after(path: &std::path::Path, watermark_ms: i64) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .map_or(true, |modified_ms| modified_ms > watermark_ms)
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
            let model_breakdowns = session_model_breakdowns(session);
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
                "modelsUsed": models_used_from_breakdowns(&model_breakdowns),
                "modelBreakdowns": model_breakdowns,
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

        let model_breakdowns = session_model_breakdowns(session);
        for model_name in models_used_from_breakdowns(&model_breakdowns) {
            if !group.models_used.contains(&model_name) {
                group.models_used.push(model_name);
            }
        }
        group.model_breakdowns.extend(model_breakdowns);
    }

    groups.into_iter().map(|(_, group)| group).collect()
}

fn model_breakdown(session: &LocalSession) -> Value {
    json!({
        "modelName": cluster_model_name_at(&session.model_name, Some(&session.date)),
        "inputTokens": session.input_tokens,
        "outputTokens": session.output_tokens,
        "cacheCreationTokens": session.cache_creation_tokens,
        "cacheReadTokens": session.cache_read_tokens,
        "cost": session.total_cost,
    })
}

fn session_model_breakdowns(session: &LocalSession) -> Vec<Value> {
    if session.model_breakdowns.is_empty() {
        return vec![model_breakdown(session)];
    }

    session
        .model_breakdowns
        .iter()
        .map(|breakdown| {
            json!({
                "modelName": cluster_model_name_at(&breakdown.model_name, Some(&session.date)),
                "inputTokens": breakdown.input_tokens,
                "outputTokens": breakdown.output_tokens,
                "cacheCreationTokens": breakdown.cache_creation_tokens,
                "cacheReadTokens": breakdown.cache_read_tokens,
                "cost": breakdown.cost,
            })
        })
        .collect()
}

fn models_used_from_breakdowns(model_breakdowns: &[Value]) -> Vec<String> {
    let mut models_used = Vec::new();
    for model_name in model_breakdowns
        .iter()
        .filter_map(|breakdown| breakdown.get("modelName").and_then(Value::as_str))
    {
        if !models_used.iter().any(|existing| existing == model_name) {
            models_used.push(model_name.to_string());
        }
    }
    models_used
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
pub(crate) fn strip_provider_prefix(model_name: &str) -> &str {
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

/// Date (YYYY-MM-DD) on/after which `ark-code-latest` is treated as
/// `deepseek-v4-flash` instead of `glm-5.2`.
const ARK_CODE_DEEPSEEK_V4_FLASH_SINCE: &str = "2026-08-01";

/// Same as [`cluster_model_name`] but lets the caller provide the record date so
/// date-based mappings (e.g. `ark-code-latest` -> `deepseek-v4-flash` after
/// Aug 1 2026) can be applied per record. `None` falls back to the current
/// (post-cutoff) behavior.
pub(crate) fn cluster_model_name_at(model_name: &str, date: Option<&str>) -> String {
    let lowered = model_name.trim().to_ascii_lowercase();
    if lowered == "zai-org/glm-5.2" || lowered == "zai-org/glm-5-2" {
        return "glm-5.2".to_string();
    }
    let stripped = strip_provider_prefix(&lowered);
    let mapped = if stripped.contains("ark-code-latest") || stripped == "ark-code" {
        if is_on_or_after(date, ARK_CODE_DEEPSEEK_V4_FLASH_SINCE) {
            "deepseek-v4-flash"
        } else {
            "glm-5.2"
        }
    } else if stripped.contains("glm-5.2") || stripped.contains("glm-5-2") {
        "glm-5.2"
    } else if stripped.contains("deepseek-v4-pro") {
        "deepseek-v4-pro"
    } else if stripped.contains("deepseek-v4-flash") {
        "deepseek-v4-flash"
    } else if stripped.contains("kimi-k2.6") {
        "kimi-k2.6"
    } else if stripped.starts_with("k3") || lowered.contains("kimi-code/") {
        "kimi-k3"
    } else if stripped.contains("qwen3.6-plus") {
        "qwen3.6-plus"
    } else if stripped.contains("step-3.7-flash") {
        "step-3.7-flash"
    } else if stripped.contains("grok4.5")
        || stripped.contains("grok-4.5")
        || stripped.contains("grok-4-5")
    {
        "grok4.5"
    } else if stripped.contains("claude-fable-5") {
        "claude-fable-5"
    } else if stripped.contains("composer-2.5-fast") || stripped.contains("composer-2-5-fast") {
        "composer-2.5-fast"
    } else if stripped.contains("composer-2.5") || stripped.contains("composer-2-5") {
        "composer-2.5"
    } else if stripped.contains("claude-sonnet-5") {
        "claude-sonnet-5"
    } else if stripped.contains("gpt-5.6") || stripped.contains("gpt-5-6") {
        // Absorb gpt-5.6 variants (e.g. `gpt-5.6-luna:medium`) into the base model.
        stripped.split(':').next().unwrap_or(stripped)
    } else {
        stripped
    };
    canonicalize_claude_4x_in_string(mapped)
}

/// Normalizes a model name by matching against known canonical model names.
/// Variants like `custom:deepseek-v4-flash` or `dqweqwe:deepseek-v4-pro21312421h43jk`
/// are merged into the canonical base model name.
pub(crate) fn cluster_model_name(model_name: &str) -> String {
    cluster_model_name_at(model_name, None)
}

/// True when `date` (YYYY-MM-DD or YYYY-MM) is on/after `cutoff` (YYYY-MM-DD).
/// A missing date is treated as before the cutoff (historical behavior).
fn is_on_or_after(date: Option<&str>, cutoff: &str) -> bool {
    let Some(date) = date else {
        return false;
    };
    let date = date.trim();
    if date.is_empty() {
        return false;
    }
    // Normalize month keys (YYYY-MM) to YYYY-MM-01 so comparison is consistent.
    let normalized = if date.len() == 7 {
        format!("{date}-01")
    } else {
        date.to_string()
    };
    normalized.as_str() >= cutoff
}

#[cfg(test)]
mod tests {
    use super::{cluster_model_name, cluster_model_name_at, LocalSource};
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
        assert_eq!(
            cluster_model_name("Grok-Composer-2.5-Fast"),
            "composer-2.5-fast"
        );
        assert_eq!(cluster_model_name("cursor-composer-2-5"), "composer-2.5");
        assert_eq!(cluster_model_name("composer-2.5"), "composer-2.5");
        assert_eq!(cluster_model_name("k3"), "kimi-k3");
        assert_eq!(cluster_model_name("k3-256k"), "kimi-k3");
        assert_eq!(cluster_model_name("kimi-code/k3"), "kimi-k3");
        assert_eq!(cluster_model_name("k3-agent"), "kimi-k3");
    }

    #[test]
    fn cluster_model_name_absorbs_gpt_56_variants() {
        assert_eq!(cluster_model_name("gpt-5.6-luna:medium"), "gpt-5.6-luna");
        assert_eq!(cluster_model_name("gpt-5.6-luna"), "gpt-5.6-luna");
        assert_eq!(cluster_model_name("gpt-5.6-sol:high"), "gpt-5.6-sol");
        assert_eq!(cluster_model_name("gpt-5.6-terra:low"), "gpt-5.6-terra");
        assert_eq!(cluster_model_name("gpt-5.6:medium"), "gpt-5.6");
        assert_eq!(cluster_model_name("openai/gpt-5.6-luna:medium"), "gpt-5.6-luna");
    }

    #[test]
    fn cluster_model_name_maps_ark_code_by_date() {
        // Before the cutoff, ark-code-latest stays glm-5.2.
        assert_eq!(
            cluster_model_name_at("ark-code-latest", Some("2026-07-31")),
            "glm-5.2"
        );
        assert_eq!(
            cluster_model_name_at("CPA/ark-code-latest", Some("2026-07-01")),
            "glm-5.2"
        );
        // On/after Aug 1 2026 it maps to deepseek-v4-flash.
        assert_eq!(
            cluster_model_name_at("ark-code-latest", Some("2026-08-01")),
            "deepseek-v4-flash"
        );
        assert_eq!(
            cluster_model_name_at("ark-code-latest", Some("2026-08-04")),
            "deepseek-v4-flash"
        );
        assert_eq!(
            cluster_model_name_at("CPA/ark-code-latest", Some("2026-08-15")),
            "deepseek-v4-flash"
        );
        // Month-precision keys are handled.
        assert_eq!(
            cluster_model_name_at("ark-code-latest", Some("2026-07")),
            "glm-5.2"
        );
        assert_eq!(
            cluster_model_name_at("ark-code-latest", Some("2026-08")),
            "deepseek-v4-flash"
        );
        // No date falls back to the pre-cutoff (historical) behavior.
        assert_eq!(cluster_model_name("ark-code-latest"), "glm-5.2");
        assert_eq!(cluster_model_name_at("ark-code-latest", None), "glm-5.2");
    }

    #[test]
    fn cluster_model_name_canonicalizes_claude_4x_versions() {
        assert_eq!(cluster_model_name("claude-opus-4.8"), "claude-opus-4-8");
        assert_eq!(cluster_model_name("claude-opus-4-8"), "claude-opus-4-8");
        assert_eq!(cluster_model_name("claude-opus-4.7"), "claude-opus-4-7");
        assert_eq!(
            cluster_model_name("anthropic/claude-opus-4.8"),
            "claude-opus-4-8"
        );
        assert_eq!(cluster_model_name("claude-sonnet-4.5"), "claude-sonnet-4-5");
        assert_eq!(
            cluster_model_name("kiro-claude-opus-4.7"),
            "kiro-claude-opus-4-7"
        );
    }

    #[test]
    fn cluster_model_name_canonicalizes_claude_sonnet_5_variants() {
        assert_eq!(cluster_model_name("claude-sonnet-5"), "claude-sonnet-5");
        assert_eq!(
            cluster_model_name("claude-sonnet-5-thinking-high"),
            "claude-sonnet-5"
        );
        assert_eq!(
            cluster_model_name("anthropic/claude-sonnet-5"),
            "claude-sonnet-5"
        );
    }

    #[test]
    fn cluster_model_name_merges_grok45_and_claude_fable_5_variants() {
        assert_eq!(cluster_model_name("grok4.5"), "grok4.5");
        assert_eq!(cluster_model_name("grok-4.5"), "grok4.5");
        assert_eq!(cluster_model_name("grok-4-5"), "grok4.5");
        assert_eq!(cluster_model_name("grok-4.5-latest"), "grok4.5");
        assert_eq!(cluster_model_name("xai/grok-4.5"), "grok4.5");
        assert_eq!(cluster_model_name("custom:grok4.5-high"), "grok4.5");
        assert_eq!(cluster_model_name("claude-fable-5"), "claude-fable-5");
        assert_eq!(
            cluster_model_name("claude-fable-5-thinking-high"),
            "claude-fable-5"
        );
        assert_eq!(
            cluster_model_name("anthropic/claude-fable-5"),
            "claude-fable-5"
        );
    }

    #[test]
    fn oh_my_pi_aliases_map_to_pi() {
        assert_eq!(LocalSource::try_from("oh-my-pi").unwrap(), LocalSource::Pi);
        assert_eq!(LocalSource::try_from("ohmypi").unwrap(), LocalSource::Pi);
        assert_eq!(LocalSource::try_from("omp").unwrap(), LocalSource::Pi);
    }

    #[test]
    fn reasonix_aliases_map_to_reasonix() {
        assert_eq!(
            LocalSource::try_from("reasonix").unwrap(),
            LocalSource::Reasonix
        );
        assert_eq!(
            LocalSource::try_from("reason-ix").unwrap(),
            LocalSource::Reasonix
        );
    }

    #[test]
    fn full_rescan_env_flag_toggles_watermark_filtering() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("TOKEN_USAGE_FULL_RESCAN");

        std::env::set_var("TOKEN_USAGE_FULL_RESCAN", "1");
        assert!(super::full_rescan_requested());
        std::env::set_var("TOKEN_USAGE_FULL_RESCAN", "yes");
        assert!(super::full_rescan_requested());
        std::env::set_var("TOKEN_USAGE_FULL_RESCAN", "0");
        assert!(!super::full_rescan_requested());
        std::env::remove_var("TOKEN_USAGE_FULL_RESCAN");
        assert!(!super::full_rescan_requested());

        match previous {
            Some(value) => std::env::set_var("TOKEN_USAGE_FULL_RESCAN", value),
            None => std::env::remove_var("TOKEN_USAGE_FULL_RESCAN"),
        }
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

pub(crate) fn iso8601_to_local_parts(text: &str) -> Option<(String, String)> {
    let parsed = DateTime::parse_from_rfc3339(text.trim()).ok()?;
    Some(local_parts(parsed.with_timezone(&Local)))
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
