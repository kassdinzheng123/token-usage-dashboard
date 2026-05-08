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
}

impl Source {
    pub const ALL: [Self; 6] = [
        Self::Claude,
        Self::Codex,
        Self::Opencode,
        Self::Hermes,
        Self::Openclaw,
        Self::Pi,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Opencode => "OpenCode",
            Self::Hermes => "Hermes",
            Self::Openclaw => "OpenClaw",
            Self::Pi => "Pi Agent",
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
            "pi" => Ok(Self::Pi),
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

pub const ALL_TASKS: [WarmTask; 19] = [
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
