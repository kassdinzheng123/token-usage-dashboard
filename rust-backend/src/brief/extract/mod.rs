pub mod claude;
pub mod codex;
pub mod cursor;
pub mod grok;
pub mod kimi;
pub mod opencode;
pub mod pi;
pub mod zcode;

use crate::protocol::Source;
use chrono::{DateTime, Local, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const MAX_USER_MESSAGES: usize = 10;
pub const MAX_USER_CHARS: usize = 800;

/// One user message, optionally stamped with the local hour it was sent.
/// Hour-scoped briefs filter on `hour`; day-level cards use all texts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimedUserText {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour: Option<i64>,
}

impl TimedUserText {
    pub fn untimed(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            hour: None,
        }
    }

    pub fn at_hour(text: impl Into<String>, hour: i64) -> Self {
        Self {
            text: text.into(),
            hour: Some(hour),
        }
    }
}

impl From<&str> for TimedUserText {
    fn from(text: &str) -> Self {
        Self::untimed(text)
    }
}

impl From<String> for TimedUserText {
    fn from(text: String) -> Self {
        Self::untimed(text)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedSession {
    pub session_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub project_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub user_texts: Vec<TimedUserText>,
    #[serde(default)]
    pub token_hint: i64,
    #[serde(default)]
    pub usage_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExtract {
    pub source: String,
    pub project: String,
    pub project_key: String,
    pub sessions: Vec<ExtractedSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceExtract {
    pub source: String,
    pub sessions: Vec<ExtractedSession>,
}

impl SourceExtract {
    pub fn has_text_content(&self) -> bool {
        self.sessions
            .iter()
            .any(|session| !session.user_texts.is_empty() || session.title.is_some())
    }

    pub fn coverage(&self) -> &'static str {
        project_coverage(&self.sessions)
    }

    pub fn projects(&self) -> Vec<ProjectExtract> {
        let mut groups: BTreeMap<String, ProjectExtract> = BTreeMap::new();
        for session in &self.sessions {
            let key = if session.project_key.trim().is_empty() {
                format!("{}:general", self.source)
            } else {
                session.project_key.clone()
            };
            let project_name = if session.project.trim().is_empty() {
                "General".to_string()
            } else {
                session.project.clone()
            };
            let entry = groups.entry(key.clone()).or_insert_with(|| ProjectExtract {
                source: self.source.clone(),
                project: project_name,
                project_key: key,
                sessions: Vec::new(),
            });
            entry.sessions.push(session.clone());
        }
        groups.into_values().collect()
    }

    pub fn to_llm_payload(&self) -> Value {
        json!({
            "source": self.source,
            "sessions": self.sessions.iter().filter(|session| {
                !session.usage_only
                    && (session.title.is_some() || !session.user_texts.is_empty())
            }).map(|session| {
                json!({
                    "sessionId": session.session_id,
                    "project": session.project,
                    "title": session.title,
                    "userTexts": plain_user_texts(&session.user_texts),
                    "tokenHint": session.token_hint,
                })
            }).collect::<Vec<_>>()
        })
    }
}

impl ProjectExtract {
    pub fn has_text_content(&self) -> bool {
        self.sessions
            .iter()
            .any(|session| !session.user_texts.is_empty() || session.title.is_some())
    }

    pub fn coverage(&self) -> &'static str {
        project_coverage(&self.sessions)
    }

    pub fn to_llm_payload(&self) -> Value {
        json!({
            "source": self.source,
            "project": self.project,
            "sessions": self.sessions.iter().filter(|session| {
                !session.usage_only
                    && (session.title.is_some() || !session.user_texts.is_empty())
            }).map(|session| {
                json!({
                    "sessionId": session.session_id,
                    "title": session.title,
                    "userTexts": plain_user_texts(&session.user_texts),
                    "tokenHint": session.token_hint,
                })
            }).collect::<Vec<_>>()
        })
    }

    pub fn card_id(&self) -> String {
        format!("{}:{}", self.source, self.project_key)
    }
}

fn project_coverage(sessions: &[ExtractedSession]) -> &'static str {
    if sessions.is_empty() {
        return "full";
    }
    let usage_only_count = sessions.iter().filter(|session| session.usage_only).count();
    if usage_only_count == sessions.len() {
        "usage_only"
    } else if usage_only_count > 0 {
        "partial"
    } else {
        "full"
    }
}

pub fn extract_for_source(
    source: Source,
    session_rows: &[Value],
) -> Result<SourceExtract, String> {
    match source {
        Source::Claude => claude::extract(session_rows),
        Source::Codex => codex::extract(session_rows),
        Source::Opencode => opencode::extract(session_rows),
        Source::Zcode => zcode::extract(session_rows),
        Source::Cursor => cursor::extract(session_rows),
        Source::Kimi => kimi::extract(session_rows),
        Source::Pi => pi::extract(session_rows),
        Source::Grok => grok::extract(session_rows),
        other => Err(format!("brief extraction not supported for {other}")),
    }
}

pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for (index, character) in text.chars().enumerate() {
        if index >= max_chars {
            break;
        }
        result.push(character);
    }
    result
}

pub fn plain_user_texts(texts: &[TimedUserText]) -> Vec<String> {
    texts.iter().map(|entry| entry.text.clone()).collect()
}

/// Cap is per hour when `hour` is known, otherwise across untimed texts.
/// This keeps later-hour messages available for hour briefs instead of
/// dropping them once the session's first 10 messages are filled.
pub fn push_capped_text(texts: &mut Vec<TimedUserText>, text: &str) {
    push_timed_text(texts, text, None);
}

pub fn push_timed_text(texts: &mut Vec<TimedUserText>, text: &str, hour: Option<i64>) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let bucket_count = match hour {
        Some(target) => texts.iter().filter(|entry| entry.hour == Some(target)).count(),
        None => texts.iter().filter(|entry| entry.hour.is_none()).count(),
    };
    if bucket_count >= MAX_USER_MESSAGES {
        return;
    }
    texts.push(TimedUserText {
        text: truncate_chars(trimmed, MAX_USER_CHARS),
        hour,
    });
}

/// Local hour (0–23) from an RFC3339 string, unix seconds, or unix millis.
pub fn local_hour_from_json(value: &Value) -> Option<i64> {
    if let Some(text) = value.as_str() {
        return local_hour_from_rfc3339(text);
    }
    let number = value.as_f64().or_else(|| value.as_i64().map(|n| n as f64))?;
    if number.abs() > 1_000_000_000_000.0 {
        local_hour_from_millis(number as i64)
    } else {
        local_hour_from_millis((number * 1000.0) as i64)
    }
}

pub fn local_hour_from_rfc3339(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Local).hour() as i64)
}

pub fn local_hour_from_millis(millis: i64) -> Option<i64> {
    Local
        .timestamp_millis_opt(millis)
        .single()
        .map(|timestamp| timestamp.hour() as i64)
        .or_else(|| {
            DateTime::<Utc>::from_timestamp_millis(millis)
                .map(|timestamp| timestamp.with_timezone(&Local).hour() as i64)
        })
}

/// Texts belonging to `hour`. If the session has no timed texts, keep them
/// only when the session maps to a single hour or this is its busiest hour —
/// otherwise multi-hour sessions would feed identical day-long prompts into
/// every hour summary.
pub fn filter_texts_for_hour(
    texts: &[TimedUserText],
    hour: i64,
    session_hours: &BTreeMap<i64, i64>,
) -> Vec<TimedUserText> {
    let matching: Vec<TimedUserText> = texts
        .iter()
        .filter(|entry| entry.hour == Some(hour))
        .cloned()
        .collect();
    if !matching.is_empty() {
        return matching;
    }
    if texts.iter().any(|entry| entry.hour.is_some()) {
        return Vec::new();
    }
    let single_hour = session_hours.len() <= 1;
    let busiest_hour = session_hours
        .iter()
        .max_by_key(|(_, tokens)| *tokens)
        .map(|(hour, _)| *hour);
    if single_hour || busiest_hour == Some(hour) {
        texts.to_vec()
    } else {
        Vec::new()
    }
}

pub fn session_token_hint(row: &Value) -> i64 {
    row.get("totalTokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            let input = row.get("inputTokens").and_then(Value::as_i64).unwrap_or(0);
            let output = row.get("outputTokens").and_then(Value::as_i64).unwrap_or(0);
            let cache_creation = row
                .get("cacheCreationTokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let cache_read = row
                .get("cacheReadTokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            Some(input + output + cache_creation + cache_read)
        })
        .unwrap_or(0)
}

pub fn session_id_of(row: &Value) -> Option<String> {
    row.get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn display_project_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "General".to_string();
    }
    PathLeaf(trimmed).to_string()
}

struct PathLeaf<'a>(&'a str);

impl std::fmt::Display for PathLeaf<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = self.0.replace('\\', "/");
        let name = value
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(self.0);
        f.write_str(name)
    }
}

pub fn decode_claude_project_dir(encoded: &str) -> String {
    // Claude encodes absolute paths by replacing `/` with `-`.
    let decoded = if encoded.starts_with('-') {
        encoded.replacen('-', "/", 1).replace('-', "/")
    } else {
        encoded.replace('-', "/")
    };
    display_project_name(&decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_texts_for_hour_keeps_only_matching_timed_texts() {
        let texts = vec![
            TimedUserText::at_hour("morning", 8),
            TimedUserText::at_hour("noon", 12),
            TimedUserText::at_hour("also morning", 8),
        ];
        let hours = BTreeMap::from([(8, 100), (12, 50)]);
        let morning = filter_texts_for_hour(&texts, 8, &hours);
        assert_eq!(
            morning.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            vec!["morning", "also morning"]
        );
        let noon = filter_texts_for_hour(&texts, 12, &hours);
        assert_eq!(
            noon.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            vec!["noon"]
        );
        assert!(filter_texts_for_hour(&texts, 9, &hours).is_empty());
    }

    #[test]
    fn filter_texts_for_hour_puts_untimed_on_busiest_hour_only() {
        let texts = vec![TimedUserText::untimed("whole day")];
        let hours = BTreeMap::from([(6, 10), (10, 500), (14, 20)]);
        assert!(filter_texts_for_hour(&texts, 6, &hours).is_empty());
        assert_eq!(
            filter_texts_for_hour(&texts, 10, &hours)
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            vec!["whole day"]
        );
        assert!(filter_texts_for_hour(&texts, 14, &hours).is_empty());
    }

    #[test]
    fn push_timed_text_caps_per_hour() {
        let mut texts = Vec::new();
        for index in 0..12 {
            push_timed_text(&mut texts, &format!("h9-{index}"), Some(9));
        }
        for index in 0..3 {
            push_timed_text(&mut texts, &format!("h10-{index}"), Some(10));
        }
        assert_eq!(texts.iter().filter(|t| t.hour == Some(9)).count(), MAX_USER_MESSAGES);
        assert_eq!(texts.iter().filter(|t| t.hour == Some(10)).count(), 3);
    }
}
