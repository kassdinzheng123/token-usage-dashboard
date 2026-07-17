pub mod claude;
pub mod codex;
pub mod cursor;
pub mod kimi;
pub mod opencode;
pub mod zcode;

use crate::protocol::Source;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const MAX_USER_MESSAGES: usize = 10;
pub const MAX_USER_CHARS: usize = 800;

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
    pub user_texts: Vec<String>,
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
                    "userTexts": session.user_texts,
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
                    "userTexts": session.user_texts,
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

pub fn push_capped_text(texts: &mut Vec<String>, text: &str) {
    if texts.len() >= MAX_USER_MESSAGES {
        return;
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    texts.push(truncate_chars(trimmed, MAX_USER_CHARS));
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
