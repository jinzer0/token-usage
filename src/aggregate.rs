use crate::{
    diagnostics::Diagnostic,
    domain::{ReasoningEffort, SessionKey, TokenStats, UsageRecord},
    scan::{ParseResult, SourceCounts},
    time::{GroupBy, TimeMatch, TimeSelection, bucket_start},
};
use anyhow::Result;
use chrono::{DateTime, Duration, Local, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct EffortStats {
    pub effort: Option<ReasoningEffort>,
    pub tokens: TokenStats,
    pub record_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelStats {
    pub model: String,
    pub tokens: TokenStats,
    pub efforts: Vec<EffortStats>,
    pub record_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionStats {
    pub key: SessionKey,
    pub parent: Option<SessionKey>,
    pub name: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub tokens: TokenStats,
    pub models: Vec<ModelStats>,
    pub record_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AggregateSnapshot {
    pub generated_at: DateTime<Utc>,
    pub sessions: Vec<SessionStats>,
    pub timeline: Vec<TimeBucket>,
    pub periods: PeriodUsage,
    pub diagnostics: Vec<Diagnostic>,
    pub source_counts: SourceCounts,
}

#[derive(Clone, Debug, Serialize)]
pub struct TimeBucket {
    pub start: DateTime<Utc>,
    pub tokens: TokenStats,
    pub record_count: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PeriodUsage {
    pub today: TokenStats,
    pub seven_days: TokenStats,
    pub thirty_days: TokenStats,
}

#[derive(Clone, Debug, Default)]
pub struct AggregateOptions {
    pub time_selection: TimeSelection,
    pub group_by: Option<GroupBy>,
}

#[derive(Debug, thiserror::Error)]
pub enum SelectorError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("ambiguous session selector {selector}; candidates: {candidates:?}")]
    Ambiguous {
        selector: String,
        candidates: Vec<String>,
    },
}

pub fn aggregate(results: Vec<ParseResult>) -> AggregateSnapshot {
    aggregate_with_options(results, &AggregateOptions::default())
        .expect("default aggregation cannot fail")
}

pub fn aggregate_with_options(
    results: Vec<ParseResult>,
    options: &AggregateOptions,
) -> Result<AggregateSnapshot> {
    let mut source_counts = SourceCounts::default();
    let mut diagnostics = Vec::new();
    let mut by_session: BTreeMap<SessionKey, Vec<UsageRecord>> = BTreeMap::new();
    let mut timeline: BTreeMap<DateTime<Utc>, TimeBucket> = BTreeMap::new();
    let mut periods = PeriodUsage::default();
    let now = Local::now();
    let today_start = crate::time::build_selection(true, None, None, now)?.since;
    let seven_days_start = (now - Duration::days(7)).with_timezone(&Utc);
    let thirty_days_start = (now - Duration::days(30)).with_timezone(&Utc);

    for result in results {
        source_counts.merge_summary(&result.summary);
        source_counts.merge_diagnostics(&result.diagnostics);
        diagnostics.extend(result.diagnostics);
        for record in result.records {
            match options.time_selection.contains(record.started_at) {
                TimeMatch::Included => {}
                TimeMatch::MissingTimestamp => {
                    source_counts.records_missing_timestamp_filtered += 1;
                    continue;
                }
                TimeMatch::OutsideRange => {
                    source_counts.records_time_filtered += 1;
                    continue;
                }
            }
            if let Some(group_by) = options.group_by
                && let Some(started_at) = record.started_at
            {
                let start = bucket_start(started_at, group_by)?;
                let bucket = timeline.entry(start).or_insert_with(|| TimeBucket {
                    start,
                    tokens: TokenStats::default(),
                    record_count: 0,
                });
                bucket.tokens.add_assign(&record.tokens);
                bucket.record_count += 1;
            } else if options.group_by.is_some() {
                source_counts.records_missing_timestamp_filtered += 1;
            }
            if let Some(started_at) = record.started_at {
                if today_start.is_some_and(|start| started_at >= start) {
                    periods.today.add_assign(&record.tokens);
                }
                if started_at >= seven_days_start {
                    periods.seven_days.add_assign(&record.tokens);
                }
                if started_at >= thirty_days_start {
                    periods.thirty_days.add_assign(&record.tokens);
                }
            }
            by_session
                .entry(record.session_key.clone())
                .or_default()
                .push(record);
        }
    }

    let mut sessions = by_session
        .into_iter()
        .map(|(key, records)| build_session(key, records))
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| {
        b.tokens
            .total_tokens
            .cmp(&a.tokens.total_tokens)
            .then_with(|| a.key.qualified().cmp(&b.key.qualified()))
    });

    Ok(AggregateSnapshot {
        generated_at: Utc::now(),
        sessions,
        timeline: timeline.into_values().collect(),
        periods,
        diagnostics,
        source_counts,
    })
}

fn build_session(key: SessionKey, records: Vec<UsageRecord>) -> SessionStats {
    let mut tokens = TokenStats::default();
    let mut parent = None;
    let mut name = None;
    let mut started_at: Option<DateTime<Utc>> = None;
    let mut models_map: BTreeMap<String, BTreeMap<Option<ReasoningEffort>, Vec<UsageRecord>>> =
        BTreeMap::new();
    let record_count = records.len() as u64;

    for record in records {
        tokens.add_assign(&record.tokens);
        parent = parent.or_else(|| record.parent_session_key.clone());
        name = name.or_else(|| record.session_name.clone());
        started_at = match (started_at, record.started_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (None, b) => b,
            (a, None) => a,
        };
        models_map
            .entry(record.model.clone())
            .or_default()
            .entry(record.reasoning_effort.clone())
            .or_default()
            .push(record);
    }

    let mut models = models_map
        .into_iter()
        .map(|(model, efforts_map)| {
            let mut model_tokens = TokenStats::default();
            let mut model_count = 0;
            let mut efforts = efforts_map
                .into_iter()
                .map(|(effort, records)| {
                    let mut effort_tokens = TokenStats::default();
                    for record in &records {
                        effort_tokens.add_assign(&record.tokens);
                    }
                    model_tokens.add_assign(&effort_tokens);
                    model_count += records.len() as u64;
                    EffortStats {
                        effort,
                        tokens: effort_tokens,
                        record_count: records.len() as u64,
                    }
                })
                .collect::<Vec<_>>();
            efforts.sort_by(|a, b| {
                b.tokens
                    .total_tokens
                    .cmp(&a.tokens.total_tokens)
                    .then_with(|| effort_label(&a.effort).cmp(&effort_label(&b.effort)))
            });
            ModelStats {
                model,
                tokens: model_tokens,
                efforts,
                record_count: model_count,
            }
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| {
        b.tokens
            .total_tokens
            .cmp(&a.tokens.total_tokens)
            .then_with(|| a.model.cmp(&b.model))
    });

    SessionStats {
        key,
        parent,
        name,
        started_at,
        tokens,
        models,
        record_count,
    }
}

fn effort_label(effort: &Option<ReasoningEffort>) -> String {
    effort
        .as_ref()
        .map(|v| v.label())
        .unwrap_or_else(|| "~none".into())
}

pub fn resolve_session<'a>(
    snapshot: &'a AggregateSnapshot,
    selector: &str,
) -> Result<&'a SessionStats, SelectorError> {
    if let Some((client, id)) = selector.split_once(':')
        && let Some(found) = snapshot
            .sessions
            .iter()
            .find(|s| s.key.client.to_string() == client && s.key.id.0 == id)
    {
        return Ok(found);
    }
    let matches = snapshot
        .sessions
        .iter()
        .filter(|s| {
            s.key.id.0 == selector
                || s.key.id.0.starts_with(selector)
                || s.name
                    .as_deref()
                    .is_some_and(|n| n == selector || n.starts_with(selector))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(SelectorError::NotFound(selector.into())),
        many => Err(SelectorError::Ambiguous {
            selector: selector.into(),
            candidates: many.iter().map(|s| s.key.qualified()).collect(),
        }),
    }
}
