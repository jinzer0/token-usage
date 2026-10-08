use chrono::{TimeZone, Utc};
use clap::Parser;
use std::{fs, path::Path};
use tempfile::tempdir;
use token_usage::{
    aggregate::{AggregateOptions, aggregate, aggregate_with_options, resolve_session},
    cli,
    domain::Client,
    sources,
    time::{self, GroupBy},
};

use token_usage::{
    aggregate::{ModelStats, SessionStats},
    domain::{ReasoningEffort, SessionId, SessionKey, TokenStats, UsageRecord},
    scan::{ParseResult, ScanSummary},
};

#[test]
fn cli_prints_version() {
    let mut cmd = assert_cmd::Command::cargo_bin("token-usage").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn codex_fixture_parses_usage_and_index_name() {
    let result = sources::codex::parse(Path::new("tests/fixtures/codex")).unwrap();
    assert_eq!(result.summary.records_emitted, 2);
    assert_eq!(
        result.records[0].session_name.as_deref(),
        Some("Codex Fixture")
    );
    assert_eq!(result.records[0].tokens.input_total, 100);
    assert_eq!(result.records[0].tokens.cache_read, 20);
    assert_eq!(result.records[0].tokens.reasoning_known, 10);
}

#[test]
fn gjc_fixture_deduplicates_and_keeps_parent() {
    let result = sources::gjc::parse(Path::new("tests/fixtures/gjc")).unwrap();
    assert_eq!(result.summary.records_emitted, 1);
    assert_eq!(result.summary.records_skipped, 1);
    assert_eq!(
        result.records[0].parent_session_key.as_ref().unwrap().id.0,
        "parent"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "GjcDuplicateMessageSkipped")
    );
}

#[test]
fn aggregate_keeps_source_qualified_sessions_separate() {
    let codex = sources::codex::parse(Path::new("tests/fixtures/codex")).unwrap();
    let gjc = sources::gjc::parse(Path::new("tests/fixtures/gjc")).unwrap();
    let snapshot = aggregate(vec![codex, gjc]);
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|s| s.key.client == Client::Codex && s.key.id.0 == "abc")
    );
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|s| s.key.client == Client::Gjc && s.key.id.0 == "same")
    );
    assert!(resolve_session(&snapshot, "codex:abc").is_ok());
}

#[test]
fn clap_accepts_json_tui_conflict_before_runtime_validation() {
    let cli = <cli::Cli as Parser>::try_parse_from(["token-usage", "--json", "--tui"]).unwrap();
    assert!(cli::validate(&cli).is_err());
}

#[test]
fn clap_rejects_bad_group_by() {
    let err = <cli::Cli as Parser>::try_parse_from(["token-usage", "--group-by", "hour"]);
    assert!(err.is_err());
}

#[test]
fn time_parser_rejects_invalid_since_and_inverted_range() {
    let now = chrono::Local
        .with_ymd_and_hms(2026, 9, 14, 12, 0, 0)
        .unwrap();
    assert!(time::build_selection(false, Some("bad"), None, now).is_err());
    assert!(time::build_selection(false, Some("2026-09-10"), Some("2026-09-01"), now).is_err());
}

#[test]
fn gjc_parser_survives_malformed_missing_timestamp_unknown_effort_and_duplicates() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    fs::write(&path, concat!(
        "{\"type\":\"session\",\"id\":\"s\",\"title\":\"Parser Torture\"}\n",
        "not-json\n",
        "{\"type\":\"thinking_level_change\",\"thinkingLevel\":\"ultra\"}\n",
        "{\"type\":\"message\",\"id\":\"a\",\"timestamp\":\"2026-09-08T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":100,\"output\":25,\"totalTokens\":125,\"reasoningTokens\":5}}}\n",
        "{\"type\":\"message\",\"id\":\"a\",\"timestamp\":\"2026-09-08T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":100,\"output\":25,\"totalTokens\":125}}}\n",
        "{\"type\":\"message\",\"id\":\"b\",\"timestamp\":\"bad\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":9007199254740991,\"output\":1,\"totalTokens\":9007199254740992}}}\n",
        "{\"type\":\"message\",\"id\":\"c\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":1,\"output\":2,\"totalTokens\":3}}}\n",
        "{\"type\":\"message\",\"id\":\"missing-model\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input\":1,\"output\":1,\"totalTokens\":2}}}\n"
    )).unwrap();

    let result = sources::gjc::parse(dir.path()).unwrap();
    assert_eq!(result.summary.records_emitted, 3);
    assert!(result.summary.records_skipped >= 3);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "GjcMalformedLine")
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "GjcInvalidTimestamp")
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "GjcDuplicateMessageSkipped")
    );
    assert_eq!(
        result.records[0].reasoning_effort.as_ref().unwrap().label(),
        "ultra"
    );
}

#[test]
fn parsers_handle_empty_and_missing_directories() {
    let dir = tempdir().unwrap();
    assert!(
        sources::gjc::parse(dir.path())
            .unwrap()
            .summary
            .empty_roots
            .contains(&dir.path().to_path_buf())
    );
    assert!(
        !sources::gjc::parse(&dir.path().join("missing"))
            .unwrap()
            .summary
            .missing_roots
            .is_empty()
    );
    fs::write(dir.path().join("empty.jsonl"), "").unwrap();
    assert_eq!(
        sources::gjc::parse(dir.path())
            .unwrap()
            .summary
            .records_emitted,
        0
    );
}

#[test]
fn codex_cumulative_reset_and_stale_are_skipped_but_later_lines_continue() {
    let dir = tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(
        sessions.join("rollout-c.jsonl"),
        concat!(
            "{\"turn_context\":{\"model\":\"m\"}}\n",
            "{\"timestamp\":\"2026-09-08T00:00:00Z\",\"token_count\":{\"total_tokens\":100}}\n",
            "{\"timestamp\":\"2026-09-08T00:01:00Z\",\"token_count\":{\"total_tokens\":90}}\n",
            "{\"timestamp\":\"2026-09-08T00:02:00Z\",\"token_count\":{\"total_tokens\":90}}\n",
            "{\"timestamp\":\"2026-09-08T00:03:00Z\",\"token_count\":{\"total_tokens\":150}}\n"
        ),
    )
    .unwrap();
    let result = sources::codex::parse(dir.path()).unwrap();
    assert_eq!(result.summary.records_emitted, 2);
    assert!(result.summary.records_skipped >= 2);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "CodexNonPositiveDelta")
    );
    assert_eq!(
        result
            .records
            .iter()
            .map(|r| r.tokens.total_tokens)
            .sum::<u64>(),
        160
    );
}

#[test]
fn time_aggregation_filters_and_groups_by_day_week_month() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("session.jsonl"), concat!(
        "{\"type\":\"session\",\"id\":\"s\"}\n",
        "{\"type\":\"message\",\"id\":\"old\",\"timestamp\":\"2026-08-30T23:59:59Z\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":1,\"output\":1,\"totalTokens\":2}}}\n",
        "{\"type\":\"message\",\"id\":\"a\",\"timestamp\":\"2026-09-01T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":10,\"output\":5,\"totalTokens\":15,\"reasoningTokens\":2}}}\n",
        "{\"type\":\"message\",\"id\":\"b\",\"timestamp\":\"2026-09-02T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":20,\"output\":5,\"totalTokens\":25,\"reasoningTokens\":3}}}\n",
        "{\"type\":\"message\",\"id\":\"no-ts\",\"message\":{\"role\":\"assistant\",\"model\":\"m\",\"usage\":{\"input\":100,\"output\":100,\"totalTokens\":200}}}\n"
    )).unwrap();
    let parsed = sources::gjc::parse(dir.path()).unwrap();
    let selection = time::build_selection(
        false,
        Some("2026-09-01"),
        Some("2026-09-02"),
        chrono::Local
            .with_ymd_and_hms(2026, 9, 14, 12, 0, 0)
            .unwrap(),
    )
    .unwrap();
    let snapshot = aggregate_with_options(
        vec![parsed],
        &AggregateOptions {
            time_selection: selection,
            group_by: Some(GroupBy::Day),
        },
    )
    .unwrap();
    assert_eq!(snapshot.sessions[0].tokens.total_tokens, 40);
    assert_eq!(snapshot.timeline.len(), 2);
    assert_eq!(
        snapshot
            .timeline
            .iter()
            .map(|b| b.tokens.total_tokens)
            .sum::<u64>(),
        40
    );
    assert_eq!(snapshot.source_counts.records_missing_timestamp_filtered, 1);
    assert_eq!(snapshot.source_counts.records_time_filtered, 1);

    let parsed = sources::gjc::parse(dir.path()).unwrap();
    let week = aggregate_with_options(
        vec![parsed],
        &AggregateOptions {
            time_selection: Default::default(),
            group_by: Some(GroupBy::Week),
        },
    )
    .unwrap();
    assert!(week.timeline.iter().any(|b| b.record_count >= 2));

    let parsed = sources::gjc::parse(dir.path()).unwrap();
    let month = aggregate_with_options(
        vec![parsed],
        &AggregateOptions {
            time_selection: Default::default(),
            group_by: Some(GroupBy::Month),
        },
    )
    .unwrap();
    assert!(month.timeline.iter().any(|b| b.tokens.total_tokens == 40));
}

#[test]
fn today_filter_uses_injected_clock_for_non_flaky_tests() {
    let now = chrono::Local
        .with_ymd_and_hms(2026, 9, 14, 12, 0, 0)
        .unwrap();
    let selection = time::build_selection(true, None, None, now).unwrap();
    assert_eq!(
        selection.contains(Some(Utc.with_ymd_and_hms(2026, 9, 14, 3, 0, 0).unwrap())),
        time::TimeMatch::Included
    );
    assert_ne!(selection.contains(None), time::TimeMatch::Included);
}

fn opencode_database(dir: &Path, input: u64, messages: usize) -> std::path::PathBuf {
    let path = dir.join("opencode.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE session (id TEXT PRIMARY KEY, parent_id TEXT, title TEXT);
        CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
        INSERT INTO session VALUES ('root', NULL, 'OpenCode Root'), ('child', 'root', 'Child');").unwrap();
    for index in 0..messages {
        let data = serde_json::json!({
            "role": "assistant", "providerID": "provider", "modelID": "model",
            "time": {"created": 1_800_000_000_000_i64, "completed": 1_800_000_000_100_i64},
            "tokens": {"input": input, "output": 0, "reasoning": 0,
                "cache": {"read": 0, "write": 0}}
        });
        db.execute(
            "INSERT INTO message VALUES (?1, 'child', 1800000000000, ?2)",
            rusqlite::params![format!("message-{index}"), data.to_string()],
        )
        .unwrap();
    }
    path
}

#[test]
fn opencode_cli_integrates_all_sources_and_root_details() {
    let dir = tempdir().unwrap();
    let path = opencode_database(dir.path(), 20, 1);
    let output = assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .args([
            "--client",
            "all",
            "--codex-home",
            "tests/fixtures/codex",
            "--gjc-home",
            "tests/fixtures/gjc",
            "--json",
            "--opencode-db",
        ])
        .arg(&path)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let sessions = json["sessions"].as_array().unwrap();
    assert!(sessions.iter().any(|s| s["key"]["client"] == "codex"));
    assert!(sessions.iter().any(|s| s["key"]["client"] == "gjc"));
    let opencode = sessions
        .iter()
        .find(|s| s["key"]["client"] == "opencode")
        .unwrap();
    assert_eq!(opencode["key"]["id"], "root");
    assert_eq!(opencode["tokens"]["total_tokens"], 20);
    assert_eq!(opencode["models"][0]["model"], "provider/model");
    assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .args(["--client", "opencode", "--opencode-db"])
        .arg(&path)
        .arg("opencode:root")
        .assert()
        .success()
        .stdout(predicates::str::contains("OpenCode Root"));
    assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .args(["--client", "opencode", "--opencode-db"])
        .arg(&path)
        .args(["--group-by", "day"])
        .assert()
        .success()
        .stdout(predicates::str::contains("20"));
}

#[test]
fn explicit_missing_database_is_diagnostic_and_unselected_source_is_not_read() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing.db");
    let output = assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .args(["--client", "opencode", "--json", "--opencode-db"])
        .arg(&missing)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["source_counts"]["errors"], 1);
    assert!(
        json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "OpenCodeMissingDatabase" && d["severity"] == "error")
    );
    assert!(!missing.exists());
    let output = assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .args([
            "--client",
            "gjc",
            "--gjc-home",
            "tests/fixtures/gjc",
            "--json",
            "--opencode-db",
        ])
        .arg(&missing)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["source_counts"]["errors"], 0);
    assert!(
        json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["client"] != "opencode")
    );
}

#[test]
fn cli_rejects_multi_message_aggregate_overflow_before_printing_snapshot() {
    let dir = tempdir().unwrap();
    let path = opencode_database(dir.path(), u64::MAX, 2);
    assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .args(["--client", "opencode", "--json", "--opencode-db"])
        .arg(path)
        .assert()
        .failure()
        .stdout("")
        .stderr(predicates::str::contains("token aggregate overflow"));
}

#[test]
fn opencode_default_paths_honor_xdg_home_and_relative_override() {
    let dir = tempdir().unwrap();
    let data = dir.path().join("data");
    let source = data.join("opencode");
    fs::create_dir_all(&source).unwrap();
    let path = opencode_database(&source, 20, 1);
    let mut default = assert_cmd::Command::cargo_bin("token-usage").unwrap();
    default
        .env("XDG_DATA_HOME", &data)
        .env_remove("OPENCODE_DB")
        .args(["--client", "opencode", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("OpenCode Root"));
    let relative = source.join("custom.db");
    fs::rename(&path, &relative).unwrap();
    for value in [std::ffi::OsStr::new("custom.db"), relative.as_os_str()] {
        assert_cmd::Command::cargo_bin("token-usage")
            .unwrap()
            .env("XDG_DATA_HOME", &data)
            .env("OPENCODE_DB", value)
            .args(["--client", "opencode", "--json"])
            .assert()
            .success()
            .stdout(predicates::str::contains("OpenCode Root"));
    }
    assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .env("XDG_DATA_HOME", &data)
        .env_remove("OPENCODE_DB")
        .args(["--client", "opencode", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"severity\": \"warning\""));
    let home_source = dir.path().join(".local/share/opencode");
    fs::create_dir_all(&home_source).unwrap();
    opencode_database(&home_source, 20, 1);
    assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .env("HOME", dir.path())
        .env_remove("XDG_DATA_HOME")
        .env_remove("OPENCODE_DB")
        .args(["--client", "opencode", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("OpenCode Root"));
    assert_cmd::Command::cargo_bin("token-usage")
        .unwrap()
        .env("OPENCODE_DB", ":memory:")
        .args(["--client", "opencode", "--json"])
        .assert()
        .failure()
        .stdout("");
}

// Numeric fixture order: total, input total, uncached, cache read, cache write,
// output total, known reasoning. Equality includes the unknown-reasoning flag.
fn daily_tokens(values: [u64; 7], unknown: bool) -> TokenStats {
    TokenStats {
        total_tokens: values[0],
        input_total: values[1],
        input_uncached: values[2],
        cache_read: values[3],
        cache_write: values[4],
        output_total: values[5],
        reasoning_known: values[6],
        reasoning_has_unknown: unknown,
    }
}

fn daily_local_noon(day: u32) -> chrono::DateTime<Utc> {
    chrono::Local
        .with_ymd_and_hms(2026, 9, day, 12, 0, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

fn daily_record(
    client: Client,
    id: &str,
    timestamp: Option<chrono::DateTime<Utc>>,
    model: &str,
    effort: Option<ReasoningEffort>,
    tokens: TokenStats,
) -> UsageRecord {
    UsageRecord {
        session_key: SessionKey {
            client,
            id: SessionId(id.into()),
        },
        parent_session_key: None,
        message_id: None,
        source_path: "daily-test".into(),
        source_line: None,
        started_at: timestamp,
        session_name: Some("Daily fixture".into()),
        model: model.into(),
        reasoning_effort: effort,
        tokens,
    }
}

fn daily_parsed(records: Vec<UsageRecord>) -> ParseResult {
    ParseResult {
        summary: ScanSummary {
            records_emitted: records.len() as u64,
            ..Default::default()
        },
        records,
        diagnostics: Vec::new(),
    }
}

type DailyExpectedModel<'a> = (
    &'a str,
    TokenStats,
    u64,
    Vec<(Option<ReasoningEffort>, TokenStats, u64)>,
);

fn daily_assert_models(actual: &[ModelStats], expected: Vec<DailyExpectedModel<'_>>) {
    assert_eq!(actual.len(), expected.len());
    for (model, (name, tokens, count, efforts)) in actual.iter().zip(expected) {
        assert_eq!(model.model, name);
        assert_eq!(model.tokens, tokens, "model {name}");
        assert_eq!(model.record_count, count, "model {name}");
        assert_eq!(model.efforts.len(), efforts.len(), "model {name}");
        for (effort, (key, tokens, count)) in model.efforts.iter().zip(efforts) {
            assert_eq!(effort.effort, key, "model {name}");
            assert_eq!(effort.tokens, tokens, "model {name}, effort {key:?}");
            assert_eq!(effort.record_count, count, "model {name}, effort {key:?}");
        }
    }
}

fn daily_assert_partition(session: &SessionStats) {
    use std::collections::BTreeMap;
    let mut tokens = TokenStats::default();
    let mut count = 0;
    let mut models = BTreeMap::new();
    let mut efforts = BTreeMap::new();
    for day in &session.daily {
        tokens.add_assign(&day.tokens);
        count += day.record_count;
        for model in &day.models {
            let entry = models
                .entry(model.model.clone())
                .or_insert((TokenStats::default(), 0));
            entry.0.add_assign(&model.tokens);
            entry.1 += model.record_count;
            for effort in &model.efforts {
                let entry = efforts
                    .entry((model.model.clone(), effort.effort.clone()))
                    .or_insert((TokenStats::default(), 0));
                entry.0.add_assign(&effort.tokens);
                entry.1 += effort.record_count;
            }
        }
    }
    assert_eq!(tokens, session.tokens);
    assert_eq!(count, session.record_count);
    assert_eq!(models.len(), session.models.len());
    for model in &session.models {
        assert_eq!(
            models.remove(&model.model),
            Some((model.tokens.clone(), model.record_count))
        );
        for effort in &model.efforts {
            assert_eq!(
                efforts.remove(&(model.model.clone(), effort.effort.clone())),
                Some((effort.tokens.clone(), effort.record_count))
            );
        }
    }
    assert!(models.is_empty());
    assert!(efforts.is_empty());
}

#[test]
fn daily_session_canonical_partition_models_efforts_and_zero_records() {
    let a = daily_tokens([10, 7, 4, 2, 1, 3, 0], false);
    let b = daily_tokens([10, 6, 3, 2, 1, 4, 0], true);
    let c = daily_tokens([10, 5, 2, 2, 1, 5, 2], false);
    let d = daily_tokens([20, 14, 8, 4, 2, 6, 3], false);
    let known_zero = daily_tokens([0; 7], false);
    let unknown_zero = daily_tokens([0; 7], true);
    let custom = Some(ReasoningEffort::Custom("ultra".into()));
    let snapshot = aggregate(vec![daily_parsed(vec![
        daily_record(
            Client::Gjc,
            "daily",
            Some(daily_local_noon(11)),
            "alpha",
            None,
            a.clone(),
        ),
        daily_record(
            Client::Gjc,
            "daily",
            Some(daily_local_noon(11)),
            "alpha",
            custom.clone(),
            b.clone(),
        ),
        daily_record(
            Client::Gjc,
            "daily",
            Some(daily_local_noon(11)),
            "beta",
            None,
            c.clone(),
        ),
        daily_record(
            Client::Gjc,
            "daily",
            Some(daily_local_noon(12)),
            "alpha",
            Some(ReasoningEffort::High),
            d.clone(),
        ),
        daily_record(
            Client::Gjc,
            "daily",
            Some(daily_local_noon(12)),
            "zero",
            None,
            known_zero.clone(),
        ),
        daily_record(
            Client::Gjc,
            "daily",
            None,
            "alpha",
            custom.clone(),
            unknown_zero.clone(),
        ),
    ])]);
    assert_eq!(snapshot.sessions.len(), 1);
    let session = &snapshot.sessions[0];
    assert_eq!(
        session.tokens,
        daily_tokens([50, 32, 17, 10, 5, 18, 5], true)
    );
    assert_eq!(session.record_count, 6);
    assert_eq!(
        session.daily.iter().map(|d| d.day).collect::<Vec<_>>(),
        vec![
            chrono::NaiveDate::from_ymd_opt(2026, 9, 12),
            chrono::NaiveDate::from_ymd_opt(2026, 9, 11),
            None,
        ]
    );
    daily_assert_models(
        &session.models,
        vec![
            (
                "alpha",
                daily_tokens([40, 27, 15, 8, 4, 13, 3], true),
                4,
                vec![
                    (Some(ReasoningEffort::High), d.clone(), 1),
                    (custom.clone(), b.clone(), 2),
                    (None, a.clone(), 1),
                ],
            ),
            ("beta", c.clone(), 1, vec![(None, c.clone(), 1)]),
            (
                "zero",
                known_zero.clone(),
                1,
                vec![(None, known_zero.clone(), 1)],
            ),
        ],
    );
    assert_eq!(session.daily[0].tokens, d);
    assert_eq!(session.daily[0].record_count, 2);
    daily_assert_models(
        &session.daily[0].models,
        vec![
            (
                "alpha",
                d.clone(),
                1,
                vec![(Some(ReasoningEffort::High), d, 1)],
            ),
            ("zero", known_zero.clone(), 1, vec![(None, known_zero, 1)]),
        ],
    );
    assert_eq!(
        session.daily[1].tokens,
        daily_tokens([30, 18, 9, 6, 3, 12, 2], true)
    );
    assert_eq!(session.daily[1].record_count, 3);
    daily_assert_models(
        &session.daily[1].models,
        vec![
            (
                "alpha",
                daily_tokens([20, 13, 7, 4, 2, 7, 0], true),
                2,
                vec![(custom.clone(), b, 1), (None, a, 1)],
            ),
            ("beta", c.clone(), 1, vec![(None, c, 1)]),
        ],
    );
    assert_eq!(session.daily[2].tokens, unknown_zero);
    assert_eq!(session.daily[2].record_count, 1);
    daily_assert_models(
        &session.daily[2].models,
        vec![(
            "alpha",
            unknown_zero.clone(),
            1,
            vec![(custom, unknown_zero, 1)],
        )],
    );
    daily_assert_partition(session);
}

#[test]
fn daily_session_three_sources_root_identity_partition_and_json_shape() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("fixture.db");
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(include_str!("fixtures/opencode/schema.sql"))
            .unwrap();
        db.execute_batch(include_str!("fixtures/opencode/usage.sql"))
            .unwrap();
    }
    let codex = sources::codex::parse(Path::new("tests/fixtures/codex")).unwrap();
    let gjc = sources::gjc::parse(Path::new("tests/fixtures/gjc")).unwrap();
    let opencode = sources::opencode::parse(&path).unwrap();
    assert_eq!(opencode.records.len(), 4);
    assert!(
        opencode
            .records
            .iter()
            .all(|r| r.session_key.qualified() == "opencode:abc")
    );
    assert!(
        opencode
            .records
            .iter()
            .all(|r| r.parent_session_key.is_none())
    );
    let snapshot = aggregate(vec![codex, gjc, opencode]);
    assert_eq!(snapshot.sessions.len(), 3);
    assert!(matches!(
        resolve_session(&snapshot, "abc"),
        Err(token_usage::aggregate::SelectorError::Ambiguous { .. })
    ));
    let codex = resolve_session(&snapshot, "codex:abc").unwrap();
    let gjc = resolve_session(&snapshot, "gjc:same").unwrap();
    let opencode = resolve_session(&snapshot, "opencode:abc").unwrap();
    let codex_tokens = daily_tokens([150, 100, 80, 20, 0, 40, 10], false);
    let gjc_tokens = daily_tokens([75, 50, 50, 0, 0, 25, 5], false);
    let opencode_tokens = daily_tokens([197, 141, 112, 24, 5, 56, 8], false);
    let anthropic = daily_tokens([192, 139, 110, 24, 5, 53, 8], false);
    let other = daily_tokens([5, 2, 2, 0, 0, 3, 0], false);
    assert_eq!(codex.tokens, codex_tokens);
    assert_eq!(codex.record_count, 2);
    assert_eq!(codex.daily.len(), 1);
    assert_eq!(codex.daily[0].day, None);
    daily_assert_models(
        &codex.daily[0].models,
        vec![(
            "gpt-5",
            codex_tokens.clone(),
            2,
            vec![(Some(ReasoningEffort::High), codex_tokens, 2)],
        )],
    );
    assert_eq!(gjc.tokens, gjc_tokens);
    assert_eq!(gjc.record_count, 1);
    assert_eq!(gjc.daily.len(), 1);
    assert_eq!(
        gjc.daily[0].day,
        Some(
            Utc.with_ymd_and_hms(2026, 9, 11, 0, 0, 0)
                .unwrap()
                .with_timezone(&chrono::Local)
                .date_naive()
        )
    );
    daily_assert_models(
        &gjc.daily[0].models,
        vec![(
            "claude",
            gjc_tokens.clone(),
            1,
            vec![(Some(ReasoningEffort::Medium), gjc_tokens, 1)],
        )],
    );
    assert_eq!(opencode.tokens, opencode_tokens);
    assert_eq!(opencode.record_count, 4);
    assert_eq!(opencode.name.as_deref(), Some("Root without usage"));
    assert_eq!(opencode.parent, None);
    assert_eq!(
        opencode.started_at,
        chrono::DateTime::<Utc>::from_timestamp_millis(0)
    );
    assert_eq!(opencode.daily.len(), 1);
    assert_eq!(
        opencode.daily[0].day,
        Some(
            chrono::DateTime::<Utc>::from_timestamp_millis(0)
                .unwrap()
                .with_timezone(&chrono::Local)
                .date_naive()
        )
    );
    daily_assert_models(
        &opencode.daily[0].models,
        vec![
            (
                "anthropic/model",
                anthropic.clone(),
                3,
                vec![(None, anthropic, 3)],
            ),
            ("other/model", other.clone(), 1, vec![(None, other, 1)]),
        ],
    );
    for session in &snapshot.sessions {
        daily_assert_partition(session);
        assert_eq!(session.daily[0].tokens, session.tokens);
        assert_eq!(session.daily[0].record_count, session.record_count);
    }

    let json = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 6);
    for field in [
        "generated_at",
        "sessions",
        "timeline",
        "periods",
        "diagnostics",
        "source_counts",
    ] {
        assert!(json.get(field).is_some(), "missing snapshot field {field}");
    }
    for (value, session) in json["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&snapshot.sessions)
    {
        assert_eq!(value.as_object().unwrap().len(), 7);
        assert!(value.get("daily").is_none());
        assert_eq!(value["key"], serde_json::to_value(&session.key).unwrap());
        assert_eq!(
            value["parent"],
            serde_json::to_value(&session.parent).unwrap()
        );
        assert_eq!(value["name"], serde_json::to_value(&session.name).unwrap());
        assert_eq!(
            value["started_at"],
            serde_json::to_value(session.started_at).unwrap()
        );
        assert_eq!(
            value["tokens"],
            serde_json::to_value(&session.tokens).unwrap()
        );
        assert_eq!(value["record_count"], session.record_count);
        assert_eq!(
            value["models"],
            serde_json::to_value(&session.models).unwrap()
        );
        let model = &value["models"][0];
        assert!(model.get("daily").is_none());
        assert_eq!(model.as_object().unwrap().len(), 4);
        assert_eq!(model["tokens"].as_object().unwrap().len(), 8);
        assert_eq!(model["efforts"][0].as_object().unwrap().len(), 3);
    }
}

#[test]
fn daily_session_time_selection_partial_day_and_grouping_undated_counter() {
    let since = daily_local_noon(11);
    let until = since + chrono::Duration::hours(2);
    let a = daily_tokens([10, 7, 4, 2, 1, 3, 0], false);
    let b = daily_tokens([10, 6, 3, 2, 1, 4, 0], true);
    let c = daily_tokens([10, 5, 2, 2, 1, 5, 2], false);
    let d = daily_tokens([20, 14, 8, 4, 2, 6, 3], false);
    let parsed = daily_parsed(vec![
        daily_record(
            Client::Gjc,
            "dated",
            Some(since - chrono::Duration::milliseconds(1)),
            "alpha",
            None,
            a,
        ),
        daily_record(Client::Gjc, "dated", Some(since), "alpha", None, b.clone()),
        daily_record(
            Client::Gjc,
            "dated",
            Some(until - chrono::Duration::milliseconds(1)),
            "beta",
            None,
            c.clone(),
        ),
        daily_record(Client::Gjc, "dated", Some(until), "alpha", None, d),
        daily_record(
            Client::Gjc,
            "undated-only",
            None,
            "zero",
            None,
            daily_tokens([0; 7], true),
        ),
    ]);
    let filtered = aggregate_with_options(
        vec![parsed.clone()],
        &AggregateOptions {
            time_selection: time::TimeSelection {
                since: Some(since),
                until: Some(until),
            },
            group_by: Some(GroupBy::Day),
        },
    )
    .unwrap();
    assert_eq!(filtered.source_counts.records_time_filtered, 2);
    assert_eq!(filtered.source_counts.records_missing_timestamp_filtered, 1);
    assert_eq!(filtered.sessions.len(), 1);
    let session = &filtered.sessions[0];
    assert_eq!(session.tokens, daily_tokens([20, 11, 5, 4, 2, 9, 2], true));
    assert_eq!(session.record_count, 2);
    assert_eq!(session.daily.len(), 1);
    assert_eq!(
        session.daily[0].day,
        chrono::NaiveDate::from_ymd_opt(2026, 9, 11)
    );
    assert_eq!(
        session.daily[0].tokens,
        daily_tokens([20, 11, 5, 4, 2, 9, 2], true)
    );
    assert_eq!(session.daily[0].record_count, 2);
    daily_assert_models(
        &session.daily[0].models,
        vec![
            ("alpha", b.clone(), 1, vec![(None, b, 1)]),
            ("beta", c.clone(), 1, vec![(None, c, 1)]),
        ],
    );
    daily_assert_partition(session);
    assert_eq!(filtered.timeline.len(), 1);
    assert_eq!(filtered.timeline[0].tokens, session.tokens);
    assert_eq!(filtered.timeline[0].record_count, 2);

    let grouped = aggregate_with_options(
        vec![parsed],
        &AggregateOptions {
            time_selection: Default::default(),
            group_by: Some(GroupBy::Day),
        },
    )
    .unwrap();
    assert_eq!(grouped.source_counts.records_time_filtered, 0);
    assert_eq!(grouped.source_counts.records_missing_timestamp_filtered, 1);
    assert_eq!(grouped.sessions.len(), 2);
    assert_eq!(grouped.timeline.len(), 1);
    assert_eq!(
        grouped.timeline[0].tokens,
        daily_tokens([50, 32, 17, 10, 5, 18, 5], true)
    );
    assert_eq!(grouped.timeline[0].record_count, 4);
    let undated = resolve_session(&grouped, "gjc:undated-only").unwrap();
    assert_eq!(undated.tokens, daily_tokens([0; 7], true));
    assert_eq!(undated.record_count, 1);
    assert_eq!(undated.daily.len(), 1);
    assert_eq!(undated.daily[0].day, None);
    daily_assert_models(
        &undated.daily[0].models,
        vec![(
            "zero",
            daily_tokens([0; 7], true),
            1,
            vec![(None, daily_tokens([0; 7], true), 1)],
        )],
    );
    for session in &grouped.sessions {
        daily_assert_partition(session);
    }
}

#[test]
fn daily_session_fixed_instants_local_midnight_and_dst() {
    const CHILD_GUARD: &str = "TOKEN_USAGE_DAILY_TIMEZONE_TEST_CHILD";
    let zone = std::env::var("TZ").unwrap_or_default();
    if !matches!(zone.as_str(), "Asia/Seoul" | "America/New_York" | "UTC") {
        assert!(
            std::env::var_os(CHILD_GUARD).is_none(),
            "timezone test child has unsupported TZ={zone:?}"
        );
        let executable = std::env::current_exe().unwrap();
        for child_zone in ["Asia/Seoul", "America/New_York", "UTC"] {
            let output = std::process::Command::new(&executable)
                .args([
                    "--exact",
                    "daily_session_fixed_instants_local_midnight_and_dst",
                    "--nocapture",
                ])
                .env("TZ", child_zone)
                .env(CHILD_GUARD, "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success(),
                "timezone test failed for TZ={child_zone}:\n{stdout}\n{stderr}"
            );
            assert!(
                stdout.contains("running 1 test") && stdout.contains("1 passed"),
                "timezone child did not execute exactly one passing test for TZ={child_zone}:\n{stdout}\n{stderr}"
            );
        }
        return;
    }
    let dates = match zone.as_str() {
        "Asia/Seoul" => [
            "2026-09-11",
            "2026-09-12",
            "2026-03-08",
            "2026-03-08",
            "2026-11-01",
            "2026-11-01",
        ],
        "America/New_York" | "UTC" => [
            "2026-09-11",
            "2026-09-11",
            "2026-03-08",
            "2026-03-08",
            "2026-11-01",
            "2026-11-01",
        ],
        other => panic!("unsupported TZ={other}; use Asia/Seoul, America/New_York or UTC"),
    };
    let instants = [
        "2026-09-11T14:59:59.999Z",
        "2026-09-11T15:00:00.001Z",
        "2026-03-08T06:59:59.999Z",
        "2026-03-08T07:00:00Z",
        "2026-11-01T05:30:00Z",
        "2026-11-01T06:30:00Z",
    ];
    let a = daily_tokens([10, 7, 4, 2, 1, 3, 0], false);
    let b = daily_tokens([10, 6, 3, 2, 1, 4, 0], true);
    let c = daily_tokens([10, 5, 2, 2, 1, 5, 2], false);
    let d = daily_tokens([20, 14, 8, 4, 2, 6, 3], false);
    let stats = [a.clone(), b.clone(), c, d, a.clone(), b.clone()];
    let records = instants
        .iter()
        .zip(&dates)
        .zip(stats)
        .map(|((instant, date), tokens)| {
            let timestamp = chrono::DateTime::parse_from_rfc3339(instant)
                .unwrap()
                .with_timezone(&Utc);
            assert_eq!(
                timestamp
                    .with_timezone(&chrono::Local)
                    .date_naive()
                    .to_string(),
                *date,
                "process timezone must match TZ={zone}"
            );
            daily_record(Client::Codex, "fixed", Some(timestamp), "m", None, tokens)
        })
        .collect();
    let snapshot = aggregate(vec![daily_parsed(records)]);
    let session = &snapshot.sessions[0];
    let mut expected = vec![("2026-11-01", daily_tokens([20, 13, 7, 4, 2, 7, 0], true), 2)];
    if zone == "Asia/Seoul" {
        expected.push(("2026-09-12", b, 1));
        expected.push(("2026-09-11", a, 1));
    } else {
        expected.push(("2026-09-11", daily_tokens([20, 13, 7, 4, 2, 7, 0], true), 2));
    }
    expected.push((
        "2026-03-08",
        daily_tokens([30, 19, 10, 6, 3, 11, 5], false),
        2,
    ));
    assert_eq!(
        session.tokens,
        daily_tokens([70, 45, 24, 14, 7, 25, 5], true)
    );
    assert_eq!(session.record_count, 6);
    assert_eq!(session.daily.len(), expected.len());
    for (day, (date, tokens, count)) in session.daily.iter().zip(expected) {
        assert_eq!(day.day.unwrap().to_string(), date);
        assert_eq!(day.tokens, tokens);
        assert_eq!(day.record_count, count);
        daily_assert_models(
            &day.models,
            vec![("m", tokens.clone(), count, vec![(None, tokens, count)])],
        );
    }
    daily_assert_partition(session);
}
