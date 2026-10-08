use crate::{
    diagnostics::Diagnostic,
    domain::{ReasoningEffort, SessionKey, TokenStats, UsageRecord},
    scan::{ParseResult, SourceCounts},
    time::{GroupBy, TimeMatch, TimeSelection, bucket_start},
};
use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
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
    #[serde(skip_serializing)]
    pub daily: Vec<SessionDayStats>,
}

#[derive(Clone, Debug)]
pub struct SessionDayStats {
    pub day: Option<NaiveDate>,
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
    let mut all = UsageAccumulator::default();
    let mut days: BTreeMap<Option<NaiveDate>, UsageAccumulator> = BTreeMap::new();
    let mut parent = None;
    let mut name = None;
    let mut started_at: Option<DateTime<Utc>> = None;

    for record in records {
        all.add_record(&record);
        let day = record
            .started_at
            .map(|t| t.with_timezone(&Local).date_naive());
        days.entry(day).or_default().add_record(&record);
        parent = parent.or_else(|| record.parent_session_key.clone());
        name = name.or_else(|| record.session_name.clone());
        started_at = match (started_at, record.started_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (None, b) => b,
            (a, None) => a,
        };
    }

    let (tokens, models, record_count) = all.finish();
    let daily = days
        .into_iter()
        .rev()
        .map(|(day, usage)| {
            let (tokens, models, record_count) = usage.finish();
            SessionDayStats {
                day,
                tokens,
                models,
                record_count,
            }
        })
        .collect();
    SessionStats {
        key,
        parent,
        name,
        started_at,
        tokens,
        models,
        record_count,
        daily,
    }
}

#[derive(Default)]
struct UsageAccumulator {
    tokens: TokenStats,
    record_count: u64,
    models: BTreeMap<String, BTreeMap<Option<ReasoningEffort>, (TokenStats, u64)>>,
}

impl UsageAccumulator {
    fn add_record(&mut self, record: &UsageRecord) {
        self.tokens.add_assign(&record.tokens);
        self.record_count += 1;
        let (tokens, count) = self
            .models
            .entry(record.model.clone())
            .or_default()
            .entry(record.reasoning_effort.clone())
            .or_default();
        tokens.add_assign(&record.tokens);
        *count += 1;
    }

    fn finish(self) -> (TokenStats, Vec<ModelStats>, u64) {
        let mut models = self
            .models
            .into_iter()
            .map(|(model, efforts_map)| {
                let mut model_tokens = TokenStats::default();
                let mut model_count = 0;
                let mut efforts = efforts_map
                    .into_iter()
                    .map(|(effort, (effort_tokens, count))| {
                        model_tokens.add_assign(&effort_tokens);
                        model_count += count;
                        EffortStats {
                            effort,
                            tokens: effort_tokens,
                            record_count: count,
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

        (self.tokens, models, self.record_count)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Client, SessionId};

    #[test]
    fn daily_keeps_local_dates_midnight_and_dst() {
        let zone = std::env::var("TZ").unwrap_or_default();
        let cases: Vec<(&str, NaiveDate)> = match zone.as_str() {
            "Asia/Seoul" => vec![
                (
                    "2026-03-10T14:59:59.999Z",
                    NaiveDate::from_ymd_opt(2026, 3, 10).unwrap(),
                ),
                (
                    "2026-03-10T15:00:00Z",
                    NaiveDate::from_ymd_opt(2026, 3, 11).unwrap(),
                ),
            ],
            "America/New_York" => vec![
                (
                    "2026-03-08T06:59:59.999Z",
                    NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
                ),
                (
                    "2026-03-08T07:00:00Z",
                    NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
                ),
                (
                    "2026-11-01T05:30:00Z",
                    NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
                ),
                (
                    "2026-11-01T06:30:00Z",
                    NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
                ),
                (
                    "2026-10-10T03:59:59.999Z",
                    NaiveDate::from_ymd_opt(2026, 10, 9).unwrap(),
                ),
                (
                    "2026-10-10T04:00:00Z",
                    NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                ),
            ],
            "America/Sao_Paulo" => vec![(
                "2018-11-04T03:30:00Z",
                NaiveDate::from_ymd_opt(2018, 11, 4).unwrap(),
            )],
            _ => {
                let time = "2026-03-10T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
                vec![(
                    "2026-03-10T12:00:00Z",
                    time.with_timezone(&Local).date_naive(),
                )]
            }
        };
        let mut result = ParseResult::empty(Client::Gjc, "timezone-fixture".into());
        let mut expected: BTreeMap<NaiveDate, u64> = BTreeMap::new();
        for (time, day) in &cases {
            *expected.entry(*day).or_default() += 1;
            result.records.push(UsageRecord {
                session_key: SessionKey {
                    client: Client::Gjc,
                    id: SessionId("timezone".into()),
                },
                parent_session_key: None,
                message_id: None,
                source_path: "timezone-fixture".into(),
                source_line: None,
                started_at: Some(time.parse().unwrap()),
                session_name: None,
                model: "timezone-model".into(),
                reasoning_effort: None,
                tokens: TokenStats {
                    total_tokens: 10,
                    input_total: 8,
                    input_uncached: 4,
                    cache_read: 3,
                    cache_write: 1,
                    output_total: 2,
                    reasoning_known: 1,
                    reasoning_has_unknown: false,
                },
            });
        }
        let snapshot = aggregate(vec![result]);
        let session = &snapshot.sessions[0];
        assert_eq!(session.daily.len(), expected.len(), "zone {zone}");
        for bucket in &session.daily {
            let count = expected.remove(&bucket.day.unwrap()).unwrap();
            assert_eq!(bucket.record_count, count);
            assert_eq!(
                bucket.tokens,
                TokenStats {
                    total_tokens: count * 10,
                    input_total: count * 8,
                    input_uncached: count * 4,
                    cache_read: count * 3,
                    cache_write: count,
                    output_total: count * 2,
                    reasoning_known: count,
                    reasoning_has_unknown: false
                }
            );
            assert_eq!(bucket.models[0].record_count, count);
        }
        assert!(expected.is_empty());
        assert_eq!(session.record_count, cases.len() as u64);
    }
}
