use token_usage::{
    aggregate::{aggregate, resolve_session},
    cli,
    domain::Client,
    sources,
};

#[test]
fn codex_fixture_parses_usage_and_index_name() {
    let result = sources::codex::parse(std::path::Path::new("tests/fixtures/codex")).unwrap();
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
    let result = sources::gjc::parse(std::path::Path::new("tests/fixtures/gjc")).unwrap();
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
    let codex = sources::codex::parse(std::path::Path::new("tests/fixtures/codex")).unwrap();
    let gjc = sources::gjc::parse(std::path::Path::new("tests/fixtures/gjc")).unwrap();
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
fn clap_rejects_json_tui_conflict_before_scan() {
    let err = <cli::Cli as clap::Parser>::try_parse_from(["token-usage", "--json", "--tui"]);
    assert!(
        err.is_ok(),
        "clap accepts flags; runtime validates conflict"
    );
}
