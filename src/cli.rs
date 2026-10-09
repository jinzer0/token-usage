use crate::{
    aggregate::{AggregateOptions, AggregateSnapshot, aggregate_with_options, resolve_session},
    diagnostics::Severity,
    domain::Client,
    format, sources,
    time::{self, GroupBy},
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
