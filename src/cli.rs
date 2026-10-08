use crate::{
    aggregate::{AggregateOptions, AggregateSnapshot, aggregate_with_options, resolve_session},
    diagnostics::Severity,
    domain::Client,
    format,
    scan::ParseResult,
    sources,
    time::{self, GroupBy, TimeMatch, TimeSelection},
};
use anyhow::{Result, bail};
use chrono::Local;
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

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
        return crate::ui::run(snapshot, || build_snapshot(&cli));
    }
    if cli.json {
        println!("{}", format::json::render(&snapshot)?);
        return Ok(());
    }
    if let Some(group_by) = cli.group_by {
        print!("{}", format::plain::timeline(&snapshot, group_by));
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
    let mut results = Vec::new();
    if matches!(cli.client, ClientFilter::All | ClientFilter::Codex) {
        let root = cli
            .codex_home
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"));
        results.push(sources::codex::parse(&root)?);
    }
    if matches!(cli.client, ClientFilter::All | ClientFilter::Gjc) {
        let root = cli.gjc_home.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".gjc/agent/sessions")
        });
        results.push(sources::gjc::parse(&root)?);
    }
    let include_opencode = matches!(cli.client, ClientFilter::All | ClientFilter::Opencode);
    if include_opencode {
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
        results.push(result);
    }
    let options = AggregateOptions {
        time_selection: time::build_selection(
            cli.today,
            cli.since.as_deref(),
            cli.until.as_deref(),
            Local::now(),
        )?,
        group_by: cli.group_by,
    };
    if include_opencode {
        check_token_totals(&results, &options.time_selection)?;
    }
    aggregate_with_options(results, &options)
}

fn check_token_totals(results: &[ParseResult], selection: &TimeSelection) -> Result<()> {
    let mut totals = [0_u64; 7];
    for record in results.iter().flat_map(|result| &result.records) {
        if !matches!(selection.contains(record.started_at), TimeMatch::Included) {
            continue;
        }
        let tokens = &record.tokens;
        let fields = [
            ("input_uncached", tokens.input_uncached),
            ("input_total", tokens.input_total),
            ("cache_read", tokens.cache_read),
            ("cache_write", tokens.cache_write),
            ("output_total", tokens.output_total),
            ("reasoning_known", tokens.reasoning_known),
            ("total_tokens", tokens.total_tokens),
        ];
        for (sum, (field, value)) in totals.iter_mut().zip(fields) {
            *sum = sum.checked_add(value).ok_or_else(|| {
                anyhow::anyhow!("token aggregate overflow in {field} with OpenCode selected")
            })?;
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SessionId, SessionKey, TokenStats, UsageRecord};
    use chrono::{DateTime, Utc};

    fn result(client: Client, tokens: TokenStats, at: Option<DateTime<Utc>>) -> ParseResult {
        let mut result = ParseResult::empty(client, PathBuf::from("fixture"));
        result.records.push(UsageRecord {
            session_key: SessionKey {
                client,
                id: SessionId("root".into()),
            },
            parent_session_key: None,
            message_id: None,
            source_path: PathBuf::from("fixture"),
            source_line: None,
            started_at: at,
            session_name: None,
            model: "provider/model".into(),
            reasoning_effort: None,
            tokens,
        });
        result
    }

    fn field_tokens(field: usize, value: u64) -> TokenStats {
        let mut tokens = TokenStats::default();
        let destination = match field {
            0 => &mut tokens.input_uncached,
            1 => &mut tokens.input_total,
            2 => &mut tokens.cache_read,
            3 => &mut tokens.cache_write,
            4 => &mut tokens.output_total,
            5 => &mut tokens.reasoning_known,
            6 => &mut tokens.total_tokens,
            _ => unreachable!(),
        };
        *destination = value;
        tokens
    }

    #[test]
    fn checked_totals_accept_max_and_reject_max_plus_one_for_all_fields() {
        for field in 0..7 {
            let mut results = vec![
                result(Client::OpenCode, field_tokens(field, u64::MAX - 1), None),
                result(Client::Gjc, field_tokens(field, 1), None),
            ];
            assert!(check_token_totals(&results, &TimeSelection::default()).is_ok());
            results.push(result(Client::Codex, field_tokens(field, 1), None));
            assert!(check_token_totals(&results, &TimeSelection::default()).is_err());
        }
    }

    #[test]
    fn checked_totals_use_the_exact_aggregation_selection() {
        let boundary = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let selection = TimeSelection {
            since: Some(boundary),
            until: None,
        };
        let results = vec![
            result(Client::OpenCode, field_tokens(6, u64::MAX), Some(boundary)),
            result(
                Client::Gjc,
                field_tokens(6, 1),
                Some(boundary - chrono::Duration::seconds(1)),
            ),
            result(Client::Codex, field_tokens(6, 1), None),
        ];
        assert!(check_token_totals(&results, &selection).is_ok());
        assert!(check_token_totals(&results, &TimeSelection::default()).is_err());
    }
}
