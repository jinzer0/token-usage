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
