// Derived from junhoyeo/tokscale, crates/tokscale-core/src/aggregator.rs,
// pinned at d4d1c751856e25913bce97bfbd7b254308863239.
// Adapted symbols: aggregate_by_session entry-fold, DailyFold::{add, finish},
// DayAccumulator::{add_message, merge, into_contribution}, and SessionAccumulator
// first/last-seen tracking. Sequential BTreeMap folds replace rayon reductions;
// UsageRecord, typed model/effort keys, local Option dates and precise timestamps
// replace UnifiedMessage/string identities/sentinels. Checked u64 explicit totals
// replace signed saturation. Cost/provider/canonicalization/intensity are omitted.
// Day merges build session totals on the real execution path. Global dates hold
// only totals and references, not another copy of model/effort details.
//
// MIT License
//
// Copyright (c) 2025 Junho Yeo
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use super::{EffortStats, ModelStats, SessionDayStats, SessionStats, checked_count, effort_label};
use crate::domain::{ReasoningEffort, SessionKey, TokenStats, UsageRecord};
use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate, Utc};
use std::collections::BTreeMap;

#[derive(Default)]
struct DayAccumulator {
    tokens: TokenStats,
    record_count: u64,
    contributions: BTreeMap<(String, Option<ReasoningEffort>), (TokenStats, u64)>,
}

impl DayAccumulator {
    fn add_message(&mut self, record: &UsageRecord) -> Result<()> {
        self.tokens.checked_add_assign(&record.tokens)?;
        checked_count(&mut self.record_count, 1, "day records")?;
        let (tokens, count) = self
            .contributions
            .entry((record.model.clone(), record.reasoning_effort.clone()))
            .or_default();
        tokens.checked_add_assign(&record.tokens)?;
        checked_count(count, 1, "model/effort records")
    }

    fn merge(&mut self, other: &Self) -> Result<()> {
        self.tokens.checked_add_assign(&other.tokens)?;
        checked_count(&mut self.record_count, other.record_count, "merged records")?;
        for (key, (tokens, count)) in &other.contributions {
            let entry = self.contributions.entry(key.clone()).or_default();
            entry.0.checked_add_assign(tokens)?;
            checked_count(&mut entry.1, *count, "merged model/effort records")?;
        }
        Ok(())
    }

    fn into_contribution(self, day: Option<NaiveDate>) -> Result<SessionDayStats> {
        let mut models: BTreeMap<String, ModelStats> = BTreeMap::new();
        for ((model, effort), (tokens, record_count)) in self.contributions {
            let entry = models.entry(model.clone()).or_insert_with(|| ModelStats {
                model,
                tokens: TokenStats::default(),
                efforts: Vec::new(),
                record_count: 0,
            });
            entry.tokens.checked_add_assign(&tokens)?;
            checked_count(&mut entry.record_count, record_count, "model records")?;
            entry.efforts.push(EffortStats {
                effort,
                tokens,
                record_count,
            });
        }
        let mut models: Vec<_> = models.into_values().collect();
        for model in &mut models {
            model.efforts.sort_by(|a, b| {
                b.tokens
                    .total_tokens
                    .cmp(&a.tokens.total_tokens)
                    .then_with(|| effort_label(&a.effort).cmp(&effort_label(&b.effort)))
            });
        }
        models.sort_by(|a, b| {
            b.tokens
                .total_tokens
                .cmp(&a.tokens.total_tokens)
                .then_with(|| a.model.cmp(&b.model))
        });
        Ok(SessionDayStats {
            day,
            tokens: self.tokens,
            models,
            record_count: self.record_count,
        })
    }
}

#[derive(Default)]
struct DailyFold {
    days: BTreeMap<Option<NaiveDate>, DayAccumulator>,
}

impl DailyFold {
    fn add(&mut self, record: &UsageRecord) -> Result<()> {
        let day = record
            .started_at
            .map(|at| at.with_timezone(&Local).date_naive());
        self.days.entry(day).or_default().add_message(record)
    }

    fn finish(self) -> Result<(SessionDayStats, Vec<SessionDayStats>)> {
        let mut all = DayAccumulator::default();
        let mut contributions = Vec::with_capacity(self.days.len());
        for (day, accumulator) in self.days.into_iter().rev() {
            all.merge(&accumulator)?;
            contributions.push(accumulator.into_contribution(day)?);
        }
        Ok((all.into_contribution(None)?, contributions))
    }
}

#[derive(Default)]
pub(super) struct SessionAccumulator {
    days: DailyFold,
    parent: Option<SessionKey>,
    name: Option<String>,
    first_seen: Option<DateTime<Utc>>,
    last_seen: Option<DateTime<Utc>>,
}

impl SessionAccumulator {
    // Called before time selection: recency includes excluded usage records.
    pub(super) fn observe_timestamp(&mut self, at: Option<DateTime<Utc>>) {
        if let Some(at) = at {
            self.last_seen = Some(self.last_seen.map_or(at, |last| last.max(at)));
        }
    }

    pub(super) fn add_message(&mut self, record: &UsageRecord) -> Result<()> {
        self.days
            .add(record)
            .with_context(|| format!("session {}", record.session_key.qualified()))?;
        self.parent = self
            .parent
            .take()
            .or_else(|| record.parent_session_key.clone());
        self.name = self.name.take().or_else(|| record.session_name.clone());
        if let Some(at) = record.started_at {
            self.first_seen = Some(self.first_seen.map_or(at, |first| first.min(at)));
        }
        Ok(())
    }

    pub(super) fn finish(self, key: SessionKey) -> Result<Option<SessionStats>> {
        if self.days.days.is_empty() {
            return Ok(None);
        }
        let (all, daily) = self
            .days
            .finish()
            .with_context(|| format!("session {}", key.qualified()))?;
        Ok(Some(SessionStats {
            key,
            parent: self.parent,
            name: self.name,
            started_at: self.first_seen,
            last_used_at: self.last_seen,
            tokens: all.tokens,
            models: all.models,
            record_count: all.record_count,
            daily,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Client, SessionId};

    fn record(total: u64, model: &str, unknown: bool) -> UsageRecord {
        UsageRecord {
            session_key: SessionKey {
                client: Client::Gjc,
                id: SessionId("s".into()),
            },
            parent_session_key: None,
            message_id: None,
            source_path: "fixture".into(),
            source_line: None,
            started_at: None,
            session_name: None,
            model: model.into(),
            reasoning_effort: Some(ReasoningEffort::Custom("exact".into())),
            tokens: TokenStats {
                total_tokens: total,
                reasoning_has_unknown: unknown,
                ..Default::default()
            },
        }
    }

    #[test]
    fn partition_merge_matches_direct_fold_without_normalizing_models_or_totals() {
        let records = [
            record(7, "Model", false),
            record(11, "model", true),
            record(0, "Model", false),
        ];
        let mut direct = DayAccumulator::default();
        let mut left = DayAccumulator::default();
        let mut right = DayAccumulator::default();
        for record in &records {
            direct.add_message(record).unwrap();
        }
        left.add_message(&records[0]).unwrap();
        for record in &records[1..] {
            right.add_message(record).unwrap();
        }
        left.merge(&right).unwrap();
        assert_eq!(left.tokens, direct.tokens);
        assert_eq!(left.contributions, direct.contributions);
        assert_eq!(left.record_count, 3);
        let finished = left.into_contribution(None).unwrap();
        assert_eq!(finished.tokens.total_tokens, 18);
        assert_eq!(finished.tokens.input_total, 0);
        assert!(finished.tokens.reasoning_has_unknown);
        assert_eq!(finished.models.len(), 2);
    }

    #[test]
    fn merge_rejects_overflow() {
        let mut left = DayAccumulator::default();
        let mut right = DayAccumulator::default();
        left.add_message(&record(u64::MAX, "m", false)).unwrap();
        right.add_message(&record(1, "m", false)).unwrap();
        assert!(left.merge(&right).is_err());
    }
}
