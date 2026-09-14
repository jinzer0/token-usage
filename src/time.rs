use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, TimeZone, Utc};
use clap::ValueEnum;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum GroupBy {
    Day,
    Week,
    Month,
}

#[derive(Clone, Debug, Default)]
pub struct TimeSelection {
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

impl TimeSelection {
    pub fn active(&self) -> bool {
        self.since.is_some() || self.until.is_some()
    }

    pub fn contains(&self, timestamp: Option<DateTime<Utc>>) -> TimeMatch {
        let Some(ts) = timestamp else {
            return if self.active() {
                TimeMatch::MissingTimestamp
            } else {
                TimeMatch::Included
            };
        };
        if self.since.is_some_and(|since| ts < since) || self.until.is_some_and(|until| ts >= until)
        {
            TimeMatch::OutsideRange
        } else {
            TimeMatch::Included
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeMatch {
    Included,
    MissingTimestamp,
    OutsideRange,
}

pub fn build_selection(
    today: bool,
    since: Option<&str>,
    until: Option<&str>,
    now: DateTime<Local>,
) -> Result<TimeSelection> {
    let mut selection = TimeSelection::default();
    if today {
        let date = now.date_naive();
        selection.since = Some(local_start_of_day(date)?);
        selection.until = Some(local_start_of_day(date + Duration::days(1))?);
    }
    if let Some(value) = since {
        selection.since = Some(parse_since(value, now)?);
    }
    if let Some(value) = until {
        selection.until = Some(parse_until(value)?);
    }
    if let (Some(since), Some(until)) = (selection.since, selection.until)
        && since >= until
    {
        bail!("--since must be earlier than --until");
    }
    Ok(selection)
}

pub fn parse_since(value: &str, now: DateTime<Local>) -> Result<DateTime<Utc>> {
    if let Some(days) = value.strip_suffix('d') {
        let days = days
            .parse::<i64>()
            .with_context(|| format!("invalid --since duration: {value}"))?;
        if days < 0 {
            bail!("invalid --since duration: {value}");
        }
        return Ok((now - Duration::days(days)).with_timezone(&Utc));
    }
    let date = parse_date(value).with_context(|| format!("invalid --since date: {value}"))?;
    local_start_of_day(date)
}

pub fn parse_until(value: &str) -> Result<DateTime<Utc>> {
    let date = parse_date(value).with_context(|| format!("invalid --until date: {value}"))?;
    local_start_of_day(date + Duration::days(1))
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    Ok(NaiveDate::parse_from_str(value, "%Y-%m-%d")?)
}

fn local_start_of_day(date: NaiveDate) -> Result<DateTime<Utc>> {
    let naive = date.and_hms_opt(0, 0, 0).context("invalid date")?;
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Ok(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(early, _) => Ok(early.with_timezone(&Utc)),
        LocalResult::None => bail!("local date does not exist: {date}"),
    }
}

pub fn bucket_start(timestamp: DateTime<Utc>, group_by: GroupBy) -> Result<DateTime<Utc>> {
    let local = timestamp.with_timezone(&Local);
    let date = local.date_naive();
    let start = match group_by {
        GroupBy::Day => date,
        GroupBy::Week => date - Duration::days(date.weekday().num_days_from_monday() as i64),
        GroupBy::Month => {
            NaiveDate::from_ymd_opt(date.year(), date.month(), 1).context("invalid month")?
        }
    };
    local_start_of_day(start)
}

pub fn bucket_label(start: DateTime<Utc>, group_by: GroupBy) -> String {
    let local = start.with_timezone(&Local).date_naive();
    match group_by {
        GroupBy::Day => local.format("%Y-%m-%d").to_string(),
        GroupBy::Week => format!("{} week", local.format("%Y-%m-%d")),
        GroupBy::Month => local.format("%Y-%m").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_duration_since() {
        let now = Local.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap();
        assert_eq!(
            parse_since("7d", now).unwrap(),
            (now - Duration::days(7)).with_timezone(&Utc)
        );
    }

    #[test]
    fn rejects_bad_since() {
        let now = Local::now();
        assert!(parse_since("seven", now).is_err());
    }

    #[test]
    fn today_is_local_day() {
        let now = Local.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap();
        let selection = build_selection(true, None, None, now).unwrap();
        assert!(selection.since.is_some());
        assert!(selection.until.is_some());
    }

    #[test]
    fn since_after_until_is_invalid() {
        let now = Local.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap();
        assert!(build_selection(false, Some("2026-09-10"), Some("2026-09-01"), now).is_err());
    }
}
