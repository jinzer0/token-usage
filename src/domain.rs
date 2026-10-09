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
    pub fn checked_add_assign(&mut self, rhs: &Self) -> anyhow::Result<()> {
        fn add(field: &str, left: u64, right: u64) -> anyhow::Result<u64> {
            left.checked_add(right).ok_or_else(|| {
                anyhow::anyhow!("token aggregate overflow in {field}: {left} + {right}")
            })
        }
        let next = Self {
            input_uncached: add("input_uncached", self.input_uncached, rhs.input_uncached)?,
            input_total: add("input_total", self.input_total, rhs.input_total)?,
            cache_read: add("cache_read", self.cache_read, rhs.cache_read)?,
            cache_write: add("cache_write", self.cache_write, rhs.cache_write)?,
            output_total: add("output_total", self.output_total, rhs.output_total)?,
            reasoning_known: add("reasoning_known", self.reasoning_known, rhs.reasoning_known)?,
            reasoning_has_unknown: self.reasoning_has_unknown | rhs.reasoning_has_unknown,
            total_tokens: add("total_tokens", self.total_tokens, rhs.total_tokens)?,
        };
        *self = next;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_add_is_atomic_for_every_numeric_field() {
        for field in 0..7 {
            let mut sum = TokenStats {
                input_uncached: 4,
                input_total: 5,
                cache_read: 6,
                cache_write: 7,
                output_total: 8,
                reasoning_known: 9,
                total_tokens: 10,
                reasoning_has_unknown: false,
            };
            let fields = [
                &mut sum.input_uncached,
                &mut sum.input_total,
                &mut sum.cache_read,
                &mut sum.cache_write,
                &mut sum.output_total,
                &mut sum.reasoning_known,
                &mut sum.total_tokens,
            ];
            *fields.into_iter().nth(field).unwrap() = u64::MAX;
            let before = sum.clone();
            let rhs = TokenStats {
                input_uncached: 1,
                input_total: 1,
                cache_read: 1,
                cache_write: 1,
                output_total: 1,
                reasoning_known: 1,
                total_tokens: 1,
                reasoning_has_unknown: true,
            };
            assert!(
                sum.checked_add_assign(&rhs)
                    .unwrap_err()
                    .to_string()
                    .contains("overflow")
            );
            assert_eq!(sum, before);
        }
    }

    #[test]
    fn checked_add_preserves_full_range_explicit_total_and_unknown_or() {
        let mut sum = TokenStats {
            total_tokens: u64::MAX - 1,
            ..Default::default()
        };
        sum.checked_add_assign(&TokenStats {
            total_tokens: 1,
            reasoning_known: 2,
            reasoning_has_unknown: true,
            ..Default::default()
        })
        .unwrap();
        sum.checked_add_assign(&TokenStats::default()).unwrap();
        assert_eq!(sum.total_tokens, u64::MAX);
        assert_eq!(sum.input_total, 0);
        assert_eq!(sum.reasoning_known, 2);
        assert!(sum.reasoning_has_unknown);
    }
}
