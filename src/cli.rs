use crate::{
    aggregate::{
        AggregateOptions, AggregateSnapshot, SessionStats, TimeBucket, aggregate_with_options,
        resolve_session,
    },
    diagnostics::Severity,
    domain::{Client, TokenStats},
    format,
    scan::ParseResult,
    sources,
    time::{self, GroupBy},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, LocalResult, TimeZone, Utc};
use clap::{Parser, ValueEnum};
use std::{collections::BTreeMap, path::PathBuf, thread};

#[derive(Debug, Parser)]
#[command(
    name = "token-usage",
    about = "Summarize local Codex, GJC, and OpenCode token usage",
    version
)]
pub struct Cli {
    pub session: Option<String>,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub verbose: bool,
    #[arg(long)]
    pub tui: bool,
    #[arg(long, value_enum, default_value_t = ClientFilter::All)]
    pub client: ClientFilter,
    #[arg(long)]
    pub codex_home: Option<PathBuf>,
    #[arg(long)]
    pub gjc_home: Option<PathBuf>,
    #[arg(long, value_name = "FILE")]
    pub opencode_db: Option<PathBuf>,
    #[arg(long)]
    pub today: bool,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub until: Option<String>,
    #[arg(long, value_enum)]
    pub group_by: Option<GroupBy>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ClientFilter {
    All,
    Codex,
    Gjc,
    Opencode,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    validate(&cli)?;
    let snapshot = build_snapshot(&cli)?;
    if cli.tui {
        let initial = match &cli.session {
            Some(selector) => Some(resolve_session(&snapshot, selector)?.key.clone()),
            None => None,
        };
        return crate::ui::run_with_initial(snapshot, initial, || build_snapshot(&cli));
    }
    if cli.json {
        println!("{}", format::json::render(&snapshot)?);
        return Ok(());
    }
    if let Some(group_by) = cli.group_by {
        if let Some(selector) = &cli.session {
            let session = resolve_session(&snapshot, selector)?;
            let scoped = session_timeline_snapshot(&snapshot, session, group_by)?;
            print!("{}", format::plain::timeline(&scoped, group_by));
        } else {
            print!("{}", format::plain::timeline(&snapshot, group_by));
        }
        return Ok(());
    }
    if let Some(selector) = &cli.session {
        let session = resolve_session(&snapshot, selector)?;
        print!("{}", format::plain::detail(session, cli.verbose));
    } else {
        print!("{}", format::plain::summary(&snapshot, cli.verbose));
    }
    Ok(())
}

pub fn validate(cli: &Cli) -> Result<()> {
    if cli.json && cli.tui {
        bail!("--json and --tui cannot be used together");
    }
    Ok(())
}

pub fn build_snapshot(cli: &Cli) -> Result<AggregateSnapshot> {
    let include_codex = matches!(cli.client, ClientFilter::All | ClientFilter::Codex);
    let include_gjc = matches!(cli.client, ClientFilter::All | ClientFilter::Gjc);
    let include_opencode = matches!(cli.client, ClientFilter::All | ClientFilter::Opencode);

    let parse_codex = || -> Result<ParseResult> {
        let root = cli
            .codex_home
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"));
        sources::codex::parse(&root)
    };
    let parse_gjc = || -> Result<ParseResult> {
        let root = cli.gjc_home.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".gjc/agent/sessions")
        });
        sources::gjc::parse(&root)
    };
    let parse_opencode = || -> Result<ParseResult> {
        let path = match &cli.opencode_db {
            Some(path) => path.clone(),
            None => sources::opencode::default_db_path()?,
        };
        let mut result = sources::opencode::parse(&path)?;
        if cli.opencode_db.is_some() {
            for diagnostic in &mut result.diagnostics {
                if diagnostic.code == "OpenCodeMissingDatabase" {
                    diagnostic.severity = Severity::Error;
                }
            }
        }
        Ok(result)
    };

    let enabled_count =
        usize::from(include_codex) + usize::from(include_gjc) + usize::from(include_opencode);
    let results = if enabled_count <= 1 {
        let mut results = Vec::with_capacity(enabled_count);
        if include_codex {
            results.push(parse_codex()?);
        }
        if include_gjc {
            results.push(parse_gjc()?);
        }
        if include_opencode {
            results.push(parse_opencode()?);
        }
        results
    } else {
        thread::scope(|scope| -> Result<Vec<ParseResult>> {
            let codex_handle = include_codex.then(|| scope.spawn(parse_codex));
            let gjc_handle = include_gjc.then(|| scope.spawn(parse_gjc));
            let opencode_result = include_opencode.then(parse_opencode);

            let mut results = Vec::with_capacity(enabled_count);
            if let Some(handle) = codex_handle {
                results.push(
                    handle
                        .join()
                        .map_err(|_| anyhow::anyhow!("Codex parser thread panicked"))??,
                );
            }
            if let Some(handle) = gjc_handle {
                results.push(
                    handle
                        .join()
                        .map_err(|_| anyhow::anyhow!("GJC parser thread panicked"))??,
                );
            }
            if let Some(result) = opencode_result {
                results.push(result?);
            }
            Ok(results)
        })?
    };
    let options = AggregateOptions {
        time_selection: time::build_selection(
            cli.today,
            cli.since.as_deref(),
            cli.until.as_deref(),
            Local::now(),
        )?,
        group_by: cli.group_by,
    };
    aggregate_with_options(results, &options)
}

impl From<ClientFilter> for Option<Client> {
    fn from(value: ClientFilter) -> Self {
        match value {
            ClientFilter::All => None,
            ClientFilter::Codex => Some(Client::Codex),
            ClientFilter::Gjc => Some(Client::Gjc),
            ClientFilter::Opencode => Some(Client::OpenCode),
        }
    }
}

fn session_timeline_snapshot(
    snapshot: &AggregateSnapshot,
    session: &SessionStats,
    group_by: GroupBy,
) -> Result<AggregateSnapshot> {
    let mut timeline: BTreeMap<DateTime<Utc>, TimeBucket> = BTreeMap::new();
    let mut missing_timestamp_records = 0u64;
    for day_stat in &session.daily {
        let Some(date) = day_stat.day else {
            missing_timestamp_records = missing_timestamp_records
                .checked_add(day_stat.record_count)
                .context("count overflow in records_missing_timestamp_filtered")?;
            continue;
        };
        let naive = date.and_hms_opt(12, 0, 0).context("invalid date")?;
        let local_dt = match Local.from_local_datetime(&naive) {
            LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt,
            LocalResult::None => bail!("local date does not exist: {date}"),
        };
        let start = time::bucket_start(local_dt.with_timezone(&Utc), group_by)?;
        let bucket = timeline.entry(start).or_insert_with(|| TimeBucket {
            start,
            tokens: TokenStats::default(),
            record_count: 0,
        });
        bucket
            .tokens
            .checked_add_assign(&day_stat.tokens)
            .context("session timeline bucket")?;
        bucket.record_count = bucket
            .record_count
            .checked_add(day_stat.record_count)
            .context("count overflow in session timeline records")?;
    }
    let mut source_counts = snapshot.source_counts.clone();
    source_counts.records_missing_timestamp_filtered = missing_timestamp_records;
    Ok(AggregateSnapshot {
        generated_at: snapshot.generated_at,
        totals: session.tokens.clone(),
        sessions: vec![session.clone()],
        dates: Vec::new(),
        timeline: timeline.into_values().collect(),
        periods: snapshot.periods.clone(),
        diagnostics: snapshot.diagnostics.clone(),
        source_counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parallel_all_client_scan_preserves_order_severity_upgrade_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let codex_home = dir.path().join("codex-missing");
        let gjc_home = dir.path().join("gjc-missing");
        let opencode_db = dir.path().join("opencode-missing.db");

        let cli = Cli::try_parse_from([
            "token-usage",
            "--client",
            "all",
            "--codex-home",
            codex_home.to_str().unwrap(),
            "--gjc-home",
            gjc_home.to_str().unwrap(),
            "--opencode-db",
            opencode_db.to_str().unwrap(),
        ])
        .unwrap();

        let snapshot = build_snapshot(&cli).unwrap();
        assert_eq!(
            snapshot.source_counts.missing_roots,
            vec![codex_home.clone(), gjc_home.clone(), opencode_db]
        );
        assert_eq!(snapshot.diagnostics.len(), 1);
        assert_eq!(snapshot.diagnostics[0].code, "OpenCodeMissingDatabase");
        assert!(matches!(snapshot.diagnostics[0].severity, Severity::Error));

        let memory_cli = Cli::try_parse_from([
            "token-usage",
            "--client",
            "all",
            "--codex-home",
            codex_home.to_str().unwrap(),
            "--gjc-home",
            gjc_home.to_str().unwrap(),
            "--opencode-db",
            Path::new(":memory:").to_str().unwrap(),
        ])
        .unwrap();
        assert!(build_snapshot(&memory_cli).is_err());
    }
}
