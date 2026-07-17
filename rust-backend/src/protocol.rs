use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Claude,
    Codex,
    Opencode,
    Hermes,
    Openclaw,
    Pi,
    Grok,
    Cursor,
    Cherry,
    ClaudeScience,
    Zcode,
    Kimi,
}

impl Source {
    pub const ALL: [Self; 12] = [
        Self::Claude,
        Self::Codex,
        Self::Opencode,
        Self::Hermes,
        Self::Openclaw,
        Self::Pi,
        Self::Grok,
        Self::Cursor,
        Self::Cherry,
        Self::ClaudeScience,
        Self::Zcode,
        Self::Kimi,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Opencode => "OpenCode",
            Self::Hermes => "Hermes",
            Self::Openclaw => "OpenClaw",
            Self::Pi => "Pi Agent",
            Self::Grok => "Grok CLI",
            Self::Cursor => "Cursor",
            Self::Cherry => "Cherry Studio",
            Self::ClaudeScience => "Claude Science",
            Self::Zcode => "ZCode",
            Self::Kimi => "Kimi",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Hermes => "hermes",
            Self::Openclaw => "openclaw",
            Self::Pi => "pi",
            Self::Grok => "grok",
            Self::Cursor => "cursor",
            Self::Cherry => "cherry",
            Self::ClaudeScience => "claude-science",
            Self::Zcode => "zcode",
            Self::Kimi => "kimi",
        })
    }
}

impl FromStr for Source {
    type Err = ParseProtocolError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "opencode" => Ok(Self::Opencode),
            "hermes" => Ok(Self::Hermes),
            "openclaw" => Ok(Self::Openclaw),
            "pi" | "oh-my-pi" | "ohmypi" | "omp" => Ok(Self::Pi),
            "grok" => Ok(Self::Grok),
            "cursor" | "cursorpp" => Ok(Self::Cursor),
            "cherry" | "cherrystudio" | "cherry-studio" => Ok(Self::Cherry),
            "claude-science" | "claude_science" | "claudescience" => Ok(Self::ClaudeScience),
            "zcode" | "z-code" | "z_code" => Ok(Self::Zcode),
            "kimi" | "kimi-code" | "kimi-work" | "kimicode" | "kimiwork" => Ok(Self::Kimi),
            _ => Err(ParseProtocolError::new("source", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    Daily,
    Monthly,
    Sessions,
    Blocks,
}

impl View {
    pub const ALL: [Self; 4] = [Self::Daily, Self::Monthly, Self::Sessions, Self::Blocks];

    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Monthly => "Monthly",
            Self::Sessions => "Sessions",
            Self::Blocks => "Blocks",
        }
    }
}

impl fmt::Display for View {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
            Self::Sessions => "sessions",
            Self::Blocks => "blocks",
        })
    }
}

impl FromStr for View {
    type Err = ParseProtocolError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "daily" => Ok(Self::Daily),
            "monthly" => Ok(Self::Monthly),
            "sessions" => Ok(Self::Sessions),
            "blocks" => Ok(Self::Blocks),
            _ => Err(ParseProtocolError::new("view", value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseProtocolError {
    field: &'static str,
    value: String,
}

impl ParseProtocolError {
    fn new(field: &'static str, value: &str) -> Self {
        Self {
            field,
            value: value.to_owned(),
        }
    }
}

impl fmt::Display for ParseProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {}: {}", self.field, self.value)
    }
}

impl std::error::Error for ParseProtocolError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarmTask {
    pub key: &'static str,
    pub source: Source,
    pub view: View,
}

impl WarmTask {
    pub fn label(self) -> String {
        format!("{} {}", self.source.label(), self.view.label())
    }
}

pub const ALL_TASKS: [WarmTask; 37] = [
    WarmTask {
        key: "claude:daily",
        source: Source::Claude,
        view: View::Daily,
    },
    WarmTask {
        key: "claude:monthly",
        source: Source::Claude,
        view: View::Monthly,
    },
    WarmTask {
        key: "claude:sessions",
        source: Source::Claude,
        view: View::Sessions,
    },
    WarmTask {
        key: "claude:blocks",
        source: Source::Claude,
        view: View::Blocks,
    },
    WarmTask {
        key: "codex:daily",
        source: Source::Codex,
        view: View::Daily,
    },
    WarmTask {
        key: "codex:monthly",
        source: Source::Codex,
        view: View::Monthly,
    },
    WarmTask {
        key: "codex:sessions",
        source: Source::Codex,
        view: View::Sessions,
    },
    WarmTask {
        key: "opencode:daily",
        source: Source::Opencode,
        view: View::Daily,
    },
    WarmTask {
        key: "opencode:monthly",
        source: Source::Opencode,
        view: View::Monthly,
    },
    WarmTask {
        key: "opencode:sessions",
        source: Source::Opencode,
        view: View::Sessions,
    },
    WarmTask {
        key: "hermes:daily",
        source: Source::Hermes,
        view: View::Daily,
    },
    WarmTask {
        key: "hermes:monthly",
        source: Source::Hermes,
        view: View::Monthly,
    },
    WarmTask {
        key: "hermes:sessions",
        source: Source::Hermes,
        view: View::Sessions,
    },
    WarmTask {
        key: "openclaw:daily",
        source: Source::Openclaw,
        view: View::Daily,
    },
    WarmTask {
        key: "openclaw:monthly",
        source: Source::Openclaw,
        view: View::Monthly,
    },
    WarmTask {
        key: "openclaw:sessions",
        source: Source::Openclaw,
        view: View::Sessions,
    },
    WarmTask {
        key: "pi:daily",
        source: Source::Pi,
        view: View::Daily,
    },
    WarmTask {
        key: "pi:monthly",
        source: Source::Pi,
        view: View::Monthly,
    },
    WarmTask {
        key: "pi:sessions",
        source: Source::Pi,
        view: View::Sessions,
    },
    WarmTask {
        key: "grok:daily",
        source: Source::Grok,
        view: View::Daily,
    },
    WarmTask {
        key: "grok:monthly",
        source: Source::Grok,
        view: View::Monthly,
    },
    WarmTask {
        key: "grok:sessions",
        source: Source::Grok,
        view: View::Sessions,
    },
    WarmTask {
        key: "cursor:daily",
        source: Source::Cursor,
        view: View::Daily,
    },
    WarmTask {
        key: "cursor:monthly",
        source: Source::Cursor,
        view: View::Monthly,
    },
    WarmTask {
        key: "cursor:sessions",
        source: Source::Cursor,
        view: View::Sessions,
    },
    WarmTask {
        key: "cherry:daily",
        source: Source::Cherry,
        view: View::Daily,
    },
    WarmTask {
        key: "cherry:monthly",
        source: Source::Cherry,
        view: View::Monthly,
    },
    WarmTask {
        key: "cherry:sessions",
        source: Source::Cherry,
        view: View::Sessions,
    },
    WarmTask {
        key: "claude-science:daily",
        source: Source::ClaudeScience,
        view: View::Daily,
    },
    WarmTask {
        key: "claude-science:monthly",
        source: Source::ClaudeScience,
        view: View::Monthly,
    },
    WarmTask {
        key: "claude-science:sessions",
        source: Source::ClaudeScience,
        view: View::Sessions,
    },
    WarmTask {
        key: "zcode:daily",
        source: Source::Zcode,
        view: View::Daily,
    },
    WarmTask {
        key: "zcode:monthly",
        source: Source::Zcode,
        view: View::Monthly,
    },
    WarmTask {
        key: "zcode:sessions",
        source: Source::Zcode,
        view: View::Sessions,
    },
    WarmTask {
        key: "kimi:daily",
        source: Source::Kimi,
        view: View::Daily,
    },
    WarmTask {
        key: "kimi:monthly",
        source: Source::Kimi,
        view: View::Monthly,
    },
    WarmTask {
        key: "kimi:sessions",
        source: Source::Kimi,
        view: View::Sessions,
    },
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmStatus {
    pub warming: bool,
    pub total: usize,
    pub completed: usize,
    pub current_key: Option<String>,
    pub current_label: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl Default for WarmStatus {
    fn default() -> Self {
        Self {
            warming: false,
            total: 0,
            completed: 0,
            current_key: None,
            current_label: None,
            started_at: None,
            finished_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub cached: usize,
    pub expected: usize,
    pub keys: Vec<String>,
    pub errors: HashMap<String, String>,
    pub warm: WarmStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefreshResponse {
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayResponse {
    pub date: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
    pub active_source_count: usize,
    pub model_count: usize,
    pub source_rows: Vec<TodaySourceRow>,
    pub model_rows: Vec<TodayModelRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaySourceRow {
    pub source: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
    pub model_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayModelRow {
    pub source: String,
    pub model_name: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyResponse {
    pub date: String,
    pub hours: Vec<HourlyRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyRow {
    pub hour: i64,
    pub source: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BriefModelInfo {
    pub base_url: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BriefModelConfig {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodayBriefCard {
    pub id: String,
    pub source: String,
    pub project: String,
    pub headline: String,
    pub bullets: Vec<String>,
    pub session_count: usize,
    pub coverage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodayBriefSection {
    pub source: String,
    pub headline: String,
    pub bullets: Vec<String>,
    pub session_count: usize,
    pub coverage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodayBriefHour {
    pub hour: i64,
    pub headline: String,
    pub session_count: usize,
    pub tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodayBriefResponse {
    pub date: String,
    pub status: String,
    pub generated_at: String,
    pub trigger: String,
    pub model: BriefModelInfo,
    pub enabled_sources: Vec<String>,
    pub content_fingerprint: String,
    /// One-line collapsed summary for the board header.
    #[serde(default)]
    pub summary: String,
    /// Project cards (CLI × project). Preferred over legacy `sections`.
    #[serde(default)]
    pub cards: Vec<TodayBriefCard>,
    /// Legacy per-CLI sections; kept for older brief files.
    #[serde(default)]
    pub sections: Vec<TodayBriefSection>,
    /// Per-hour timeline of today's activity. Absent in briefs saved before
    /// this field existed; those regenerate once on the next request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<Vec<TodayBriefHour>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TodayBriefResponse {
    pub fn normalized(mut self) -> Self {
        if self.cards.is_empty() && !self.sections.is_empty() {
            self.cards = self
                .sections
                .iter()
                .map(|section| TodayBriefCard {
                    id: format!("{}:{}", section.source, section.headline),
                    source: section.source.clone(),
                    project: section.source.clone(),
                    headline: section.headline.clone(),
                    bullets: section.bullets.clone(),
                    session_count: section.session_count,
                    coverage: section.coverage.clone(),
                })
                .collect();
        }
        if self.summary.trim().is_empty() && !self.cards.is_empty() {
            self.summary = build_board_summary(&self.cards);
        }
        self
    }
}

pub fn build_board_summary(cards: &[TodayBriefCard]) -> String {
    if cards.is_empty() {
        return "今日暂无项目摘要".to_string();
    }
    let previews: Vec<String> = cards
        .iter()
        .take(3)
        .map(|card| format!("{}·{}", short_source(&card.source), card.project))
        .collect();
    if cards.len() <= 3 {
        format!("{} 个项目：{}", cards.len(), previews.join("；"))
    } else {
        format!(
            "{} 个项目：{} 等",
            cards.len(),
            previews.join("；")
        )
    }
}

fn short_source(source: &str) -> &str {
    match source {
        "claude" => "Claude",
        "codex" => "Codex",
        "cursor" => "Cursor",
        "zcode" => "ZCode",
        "kimi" => "Kimi",
        other => other,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefGenerateRequest {
    #[serde(default)]
    pub force: Option<bool>,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<BriefModelConfig>,
}

#[cfg(test)]
mod tests {
    use super::Source;
    use std::str::FromStr;

    #[test]
    fn oh_my_pi_aliases_parse_as_pi() {
        assert_eq!(Source::from_str("oh-my-pi").unwrap(), Source::Pi);
        assert_eq!(Source::from_str("ohmypi").unwrap(), Source::Pi);
        assert_eq!(Source::from_str("omp").unwrap(), Source::Pi);
    }

    #[test]
    fn kimi_aliases_parse_as_kimi() {
        assert_eq!(Source::from_str("kimi").unwrap(), Source::Kimi);
        assert_eq!(Source::from_str("kimi-code").unwrap(), Source::Kimi);
        assert_eq!(Source::from_str("kimi-work").unwrap(), Source::Kimi);
        assert_eq!(Source::from_str("kimicode").unwrap(), Source::Kimi);
        assert_eq!(Source::from_str("kimiwork").unwrap(), Source::Kimi);
    }
}
