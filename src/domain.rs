use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{fmt, path::PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    Codex,
    Gjc,
    OpenCode,
}

impl fmt::Display for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Client::Codex => "codex",
            Client::Gjc => "gjc",
            Client::OpenCode => "opencode",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct SessionId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct MessageId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct SessionKey {
    pub client: Client,
    pub id: SessionId,
}

impl SessionKey {
    pub fn qualified(&self) -> String {
        format!("{}:{}", self.client, self.id.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    Custom(String),
}

impl ReasoningEffort {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "none" | "off" | "minimal" => None,
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" | "xhigh" | "max" => Some(Self::High),
            other => Some(Self::Custom(other.to_string())),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Low => "low".into(),
            Self::Medium => "medium".into(),
            Self::High => "high".into(),
            Self::Custom(v) => v.clone(),
        }
    }
}

#[derive(Default, Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TokenStats {
    pub input_uncached: u64,
    pub input_total: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output_total: u64,
    pub reasoning_known: u64,
    pub reasoning_has_unknown: bool,
    pub total_tokens: u64,
}

impl TokenStats {
    pub fn add_assign(&mut self, rhs: &Self) {
        self.input_uncached += rhs.input_uncached;
        self.input_total += rhs.input_total;
        self.cache_read += rhs.cache_read;
        self.cache_write += rhs.cache_write;
        self.output_total += rhs.output_total;
        self.reasoning_known += rhs.reasoning_known;
        self.reasoning_has_unknown |= rhs.reasoning_has_unknown;
        self.total_tokens += rhs.total_tokens;
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct UsageRecord {
    pub session_key: SessionKey,
    pub parent_session_key: Option<SessionKey>,
    pub message_id: Option<MessageId>,
    pub source_path: PathBuf,
    pub source_line: Option<u64>,
    pub started_at: Option<DateTime<Utc>>,
    pub session_name: Option<String>,
    pub model: String,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub tokens: TokenStats,
}
