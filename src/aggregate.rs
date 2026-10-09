use crate::{
    diagnostics::{Diagnostic, Severity},
    domain::{ReasoningEffort, SessionKey, TokenStats},
    scan::{ParseResult, SourceCounts},
    time::{GroupBy, TimeMatch, TimeSelection, bucket_start},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

mod tokscale;

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
    pub last_used_at: Option<DateTime<Utc>>,
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
    pub totals: TokenStats,
    pub sessions: Vec<SessionStats>,
    #[serde(skip_serializing)]
    pub dates: Vec<DateStats>,
    pub timeline: Vec<TimeBucket>,
    pub periods: PeriodUsage,
    pub diagnostics: Vec<Diagnostic>,
    pub source_counts: SourceCounts,
}

#[derive(Clone, Debug)]
pub struct SessionDayRef {
    pub session_index: usize,
    pub day_index: usize,
}

#[derive(Clone, Debug)]
pub struct DateStats {
    pub day: Option<NaiveDate>,
    pub tokens: TokenStats,
    pub record_count: u64,
    pub sessions: Vec<SessionDayRef>,
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

pub fn aggregate(results: Vec<ParseResult>) -> Result<AggregateSnapshot> {
    aggregate_with_options(results, &AggregateOptions::default())
}

pub fn aggregate_with_options(
    results: Vec<ParseResult>,
    options: &AggregateOptions,
) -> Result<AggregateSnapshot> {
    let mut source_counts = SourceCounts::default();
    let mut diagnostics = Vec::new();
    let mut by_session: BTreeMap<SessionKey, tokscale::SessionAccumulator> = BTreeMap::new();
    let mut timeline: BTreeMap<DateTime<Utc>, TimeBucket> = BTreeMap::new();
    let mut periods = PeriodUsage::default();
    let now = Local::now();
    let today_start = crate::time::build_selection(true, None, None, now)?.since;
    let seven_days_start = now
        .checked_sub_signed(Duration::days(7))
        .context("seven-day period start overflow")?
        .with_timezone(&Utc);
    let thirty_days_start = now
        .checked_sub_signed(Duration::days(30))
        .context("thirty-day period start overflow")?
        .with_timezone(&Utc);

    for result in results {
        merge_source_counts(&mut source_counts, &result)?;
        diagnostics.extend(result.diagnostics);
        for record in result.records {
            let session = by_session.entry(record.session_key.clone()).or_default();
            session.observe_timestamp(record.started_at);
            match options.time_selection.contains(record.started_at) {
                TimeMatch::Included => {}
                TimeMatch::MissingTimestamp => {
                    checked_count(
                        &mut source_counts.records_missing_timestamp_filtered,
                        1,
                        "missing timestamp filtered",
                    )?;
                    continue;
                }
                TimeMatch::OutsideRange => {
                    checked_count(&mut source_counts.records_time_filtered, 1, "time filtered")?;
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
                bucket
                    .tokens
                    .checked_add_assign(&record.tokens)
                    .context("timeline bucket")?;
                checked_count(&mut bucket.record_count, 1, "timeline records")?;
            } else if options.group_by.is_some() {
                checked_count(
                    &mut source_counts.records_missing_timestamp_filtered,
                    1,
                    "missing timestamp filtered",
                )?;
            }
            if let Some(started_at) = record.started_at {
                if today_start.is_some_and(|start| started_at >= start) {
                    periods
                        .today
                        .checked_add_assign(&record.tokens)
                        .context("today period")?;
                }
                if started_at >= seven_days_start {
                    periods
                        .seven_days
                        .checked_add_assign(&record.tokens)
                        .context("seven-day period")?;
                }
                if started_at >= thirty_days_start {
                    periods
                        .thirty_days
                        .checked_add_assign(&record.tokens)
                        .context("thirty-day period")?;
                }
            }
            session.add_message(&record)?;
        }
    }

    let mut sessions = by_session
        .into_iter()
        .map(|(key, accumulator)| accumulator.finish(key))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| {
        b.tokens
            .total_tokens
            .cmp(&a.tokens.total_tokens)
            .then_with(|| a.key.qualified().cmp(&b.key.qualified()))
    });

    let mut totals = TokenStats::default();
    let mut dates: BTreeMap<Option<NaiveDate>, DateStats> = BTreeMap::new();
    for (session_index, session) in sessions.iter().enumerate() {
        totals
            .checked_add_assign(&session.tokens)
            .context("snapshot totals")?;
        for (day_index, day) in session.daily.iter().enumerate() {
            let date = dates.entry(day.day).or_insert_with(|| DateStats {
                day: day.day,
                tokens: TokenStats::default(),
                record_count: 0,
                sessions: Vec::new(),
            });
            date.tokens
                .checked_add_assign(&day.tokens)
                .context("global date totals")?;
            checked_count(
                &mut date.record_count,
                day.record_count,
                "global date records",
            )?;
            date.sessions.push(SessionDayRef {
                session_index,
                day_index,
            });
        }
    }
    Ok(AggregateSnapshot {
        generated_at: Utc::now(),
        totals,
        sessions,
        dates: dates.into_values().rev().collect(),
        timeline: timeline.into_values().collect(),
        periods,
        diagnostics,
        source_counts,
    })
}

fn checked_count(sum: &mut u64, value: u64, field: &str) -> Result<()> {
    *sum = sum
        .checked_add(value)
        .ok_or_else(|| anyhow::anyhow!("count overflow in {field}: {sum} + {value}"))?;
    Ok(())
}

fn merge_source_counts(counts: &mut SourceCounts, result: &ParseResult) -> Result<()> {
    let summary = &result.summary;
    counts.roots_scanned.extend(summary.roots_scanned.clone());
    counts.missing_roots.extend(summary.missing_roots.clone());
    counts.empty_roots.extend(summary.empty_roots.clone());
    checked_count(
        &mut counts.files_scanned,
        summary.files_scanned,
        "files_scanned",
    )?;
    checked_count(&mut counts.lines_read, summary.lines_read, "lines_read")?;
    checked_count(
        &mut counts.records_emitted,
        summary.records_emitted,
        "records_emitted",
    )?;
    checked_count(
        &mut counts.records_skipped,
        summary.records_skipped,
        "records_skipped",
    )?;
    checked_count(
        &mut counts.records_time_filtered,
        summary.records_time_filtered,
        "records_time_filtered",
    )?;
    checked_count(
        &mut counts.records_missing_timestamp_filtered,
        summary.records_missing_timestamp_filtered,
        "records_missing_timestamp_filtered",
    )?;
    for diagnostic in &result.diagnostics {
        match diagnostic.severity {
            Severity::Warning => checked_count(&mut counts.warnings, 1, "warnings")?,
            Severity::Error => checked_count(&mut counts.errors, 1, "errors")?,
        }
    }
    Ok(())
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
    use crate::domain::{Client, SessionId, UsageRecord};

    fn fixture_record(
        client: Client,
        id: &str,
        at: Option<DateTime<Utc>>,
        tokens: TokenStats,
    ) -> UsageRecord {
        UsageRecord {
            session_key: SessionKey {
                client,
                id: SessionId(id.into()),
            },
            parent_session_key: None,
            message_id: None,
            source_path: "fixture".into(),
            source_line: None,
            started_at: at,
            session_name: None,
            model: "raw-model".into(),
            reasoning_effort: None,
            tokens,
        }
    }

    fn fixture(records: Vec<UsageRecord>) -> ParseResult {
        let mut result = ParseResult::empty(Client::Gjc, "fixture".into());
        result.records = records;
        result
    }

    fn field_tokens(field: usize, value: u64) -> TokenStats {
        let mut tokens = TokenStats::default();
        let fields = [
            &mut tokens.input_uncached,
            &mut tokens.input_total,
            &mut tokens.cache_read,
            &mut tokens.cache_write,
            &mut tokens.output_total,
            &mut tokens.reasoning_known,
            &mut tokens.total_tokens,
        ];
        *fields.into_iter().nth(field).unwrap() = value;
        tokens
    }

    #[test]
    fn all_clients_accept_max_and_reject_max_plus_one_in_every_field() {
        for client in [Client::Codex, Client::Gjc, Client::OpenCode] {
            for field in 0..7 {
                let mut records = vec![
                    fixture_record(client, "one", None, field_tokens(field, u64::MAX - 1)),
                    fixture_record(client, "two", None, field_tokens(field, 1)),
                ];
                assert!(aggregate(vec![fixture(records.clone())]).is_ok());
                records.push(fixture_record(
                    client,
                    "three",
                    None,
                    field_tokens(field, 1),
                ));
                assert!(aggregate(vec![fixture(records)]).is_err());
            }
        }
    }

    #[test]
    fn recency_is_pre_filter_but_usage_and_start_are_filtered() {
        let early: DateTime<Utc> = "2026-03-10T12:00:00.123456789Z".parse().unwrap();
        let late = early + Duration::days(2);
        let tokens = TokenStats {
            total_tokens: 7,
            ..Default::default()
        };
        let result = fixture(vec![
            fixture_record(Client::Gjc, "kept", Some(early), tokens.clone()),
            fixture_record(Client::Gjc, "kept", Some(late), field_tokens(6, u64::MAX)),
            fixture_record(Client::Gjc, "removed", Some(late), tokens),
            fixture_record(Client::Gjc, "kept", None, field_tokens(6, 1)),
        ]);
        let snapshot = aggregate_with_options(
            vec![result],
            &AggregateOptions {
                time_selection: TimeSelection {
                    since: Some(early),
                    until: Some(early + Duration::seconds(1)),
                },
                group_by: Some(GroupBy::Day),
            },
        )
        .unwrap();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].started_at, Some(early));
        assert_eq!(snapshot.sessions[0].last_used_at, Some(late));
        assert_eq!(snapshot.totals.total_tokens, 7);
        assert_eq!(snapshot.timeline[0].tokens.total_tokens, 7);
        assert_eq!(snapshot.source_counts.records_time_filtered, 2);
        assert_eq!(snapshot.source_counts.records_missing_timestamp_filtered, 1);
    }

    #[test]
    fn global_dates_reference_sorted_sessions_and_conserve_unknown_and_zero_records() {
        let at: DateTime<Utc> = "2026-03-10T12:00:00Z".parse().unwrap();
        let snapshot = aggregate(vec![fixture(vec![
            fixture_record(Client::Gjc, "low", Some(at), field_tokens(6, 3)),
            fixture_record(Client::Codex, "high", Some(at), field_tokens(6, 9)),
            fixture_record(
                Client::Gjc,
                "low",
                None,
                TokenStats {
                    reasoning_has_unknown: true,
                    ..Default::default()
                },
            ),
            fixture_record(Client::OpenCode, "zero", None, TokenStats::default()),
        ])])
        .unwrap();
        assert_eq!(snapshot.sessions[0].key.id.0, "high");
        assert_eq!(snapshot.dates.len(), 2);
        assert_eq!(
            snapshot.dates[0].day,
            Some(at.with_timezone(&Local).date_naive())
        );
        assert_eq!(snapshot.dates[1].day, None);
        assert_eq!(snapshot.dates[1].record_count, 2);
        assert!(snapshot.dates[1].tokens.reasoning_has_unknown);
        let mut total = TokenStats::default();
        for date in &snapshot.dates {
            total.checked_add_assign(&date.tokens).unwrap();
            let mut day_total = TokenStats::default();
            let mut count = 0;
            for reference in &date.sessions {
                let day = &snapshot.sessions[reference.session_index].daily[reference.day_index];
                assert_eq!(day.day, date.day);
                day_total.checked_add_assign(&day.tokens).unwrap();
                count += day.record_count;
            }
            assert_eq!(day_total, date.tokens);
            assert_eq!(count, date.record_count);
        }
        assert_eq!(total, snapshot.totals);
        assert_eq!(total.total_tokens, 12);
        assert!(total.reasoning_has_unknown);
    }

    #[test]
    fn summary_and_count_overflows_are_errors() {
        let mut first = fixture(Vec::new());
        first.summary.records_emitted = u64::MAX;
        let mut second = fixture(Vec::new());
        second.summary.records_emitted = 1;
        assert!(aggregate(vec![first, second]).is_err());
        let mut count = u64::MAX;
        assert!(checked_count(&mut count, 1, "test records").is_err());
        assert_eq!(count, u64::MAX);
    }

    #[test]
    fn timeline_and_period_overflows_propagate_context() {
        let at = Utc::now() + Duration::seconds(1);
        let records = vec![
            fixture_record(Client::Codex, "one", Some(at), field_tokens(6, u64::MAX)),
            fixture_record(Client::Gjc, "two", Some(at), field_tokens(6, 1)),
        ];
        let timeline_error = aggregate_with_options(
            vec![fixture(records.clone())],
            &AggregateOptions {
                group_by: Some(GroupBy::Day),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(timeline_error.to_string(), "timeline bucket");
        assert!(format!("{timeline_error:#}").contains("total_tokens"));
        let period_error = aggregate(vec![fixture(records)]).unwrap_err();
        assert_eq!(period_error.to_string(), "today period");
        assert!(format!("{period_error:#}").contains("total_tokens"));
    }

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
        let snapshot = aggregate(vec![result]).unwrap();
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
