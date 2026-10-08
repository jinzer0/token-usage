use chrono::{Local, TimeZone, Utc};
use clap::Parser;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::{TempDir, tempdir};
use token_usage::{
    aggregate::{AggregateOptions, aggregate, aggregate_with_options, resolve_session},
    cli,
    diagnostics::Severity,
    domain::Client,
    scan::ParseResult,
    sources,
    time::{self, GroupBy},
};

const SCHEMA: &str = include_str!("fixtures/opencode/schema.sql");
const USAGE: &str = include_str!("fixtures/opencode/usage.sql");

fn database(usage: bool) -> (TempDir, PathBuf, Connection) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(SCHEMA).unwrap();
    if usage {
        db.execute_batch(USAGE).unwrap();
    }
    (dir, path, db)
}

fn session(db: &Connection, id: &str, parent: Option<&str>) {
    db.execute(
        "INSERT INTO session(id, parent_id, title) VALUES (?1, ?2, ?1)",
        params![id, parent],
    )
    .unwrap();
}

fn assistant() -> Value {
    json!({
        "role": "assistant", "providerID": "provider", "modelID": "model",
        "time": {"created": 0},
        "tokens": {"input": 10, "output": 5, "reasoning": 2,
                   "cache": {"read": 3, "write": 1}}
    })
}

fn message(db: &Connection, id: &str, session_id: &str, value: Value) {
    raw_message(db, id, session_id, &value.to_string());
}

fn raw_message(db: &Connection, id: &str, session_id: &str, data: &str) {
    db.execute(
        "INSERT INTO message(id, session_id, time_created, data) VALUES (?1, ?2, 0, ?3)",
        params![id, session_id, data],
    )
    .unwrap();
}

fn assert_diagnostic_paths(result: &ParseResult, path: &Path) {
    assert!(!result.diagnostics.is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| { d.client == Client::OpenCode && d.path.as_deref() == Some(path) })
    );
}

#[test]
fn fixture_normalizes_components_and_roots_without_counting_parts_or_summaries() {
    let (_dir, path, db) = database(true);
    drop(db);
    let result = sources::opencode::parse(&path).unwrap();
    assert_eq!(result.summary.client, Some(Client::OpenCode));
    assert_eq!(result.summary.records_emitted, 4);
    assert_eq!(result.records.len(), 4);
    assert!(result.records.iter().all(|r| {
        r.session_key.client == Client::OpenCode
            && r.session_key.id.0 == "abc"
            && r.parent_session_key.is_none()
            && r.session_name.as_deref() == Some("Root without usage")
            && r.source_path == path
    }));
    let first = result
        .records
        .iter()
        .find(|r| r.message_id.as_ref().unwrap().0 == "first")
        .unwrap();
    assert_eq!(first.tokens.input_uncached, 100);
    assert_eq!(first.tokens.input_total, 123);
    assert_eq!(first.tokens.cache_read, 20);
    assert_eq!(first.tokens.cache_write, 3);
    assert_eq!(first.tokens.output_total, 47);
    assert_eq!(first.tokens.reasoning_known, 7);
    assert!(!first.tokens.reasoning_has_unknown);
    assert_eq!(first.tokens.total_tokens, 170);
    assert_eq!(first.started_at, Some(Utc.timestamp_millis_opt(0).unwrap()));
    let snapshot = aggregate(vec![result]);
    assert_eq!(snapshot.sessions.len(), 1);
    let root = resolve_session(&snapshot, "opencode:abc").unwrap();
    assert_eq!(root.record_count, 4);
    assert_eq!(root.tokens.input_uncached, 112);
    assert_eq!(root.tokens.input_total, 141);
    assert_eq!(root.tokens.cache_read, 24);
    assert_eq!(root.tokens.cache_write, 5);
    assert_eq!(root.tokens.output_total, 56);
    assert_eq!(root.tokens.reasoning_known, 8);
    assert_eq!(root.tokens.total_tokens, 197);
    assert_eq!(root.models.len(), 2);
    assert_eq!(
        root.models
            .iter()
            .find(|m| m.model == "anthropic/model")
            .unwrap()
            .tokens
            .total_tokens,
        192
    );
}

#[test]
fn assistant_compaction_usage_is_real_usage_not_a_session_summary() {
    let (_dir, path, db) = database(false);
    session(&db, "root", None);
    let mut data = assistant();
    data["summary"] = json!(true);
    message(&db, "compaction", "root", data);
    let result = sources::opencode::parse(&path).unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].tokens.total_tokens, 21);
}

#[test]
fn source_neutral_aggregation_keeps_equal_session_ids_separate() {
    let (_dir, path, _db) = database(true);
    let snapshot = aggregate(vec![
        sources::opencode::parse(&path).unwrap(),
        sources::codex::parse(Path::new("tests/fixtures/codex")).unwrap(),
        sources::gjc::parse(Path::new("tests/fixtures/gjc")).unwrap(),
    ]);
    assert_eq!(
        resolve_session(&snapshot, "opencode:abc")
            .unwrap()
            .tokens
            .total_tokens,
        197
    );
    assert!(resolve_session(&snapshot, "codex:abc").is_ok());
    assert!(resolve_session(&snapshot, "abc").is_err());
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|s| s.key.client == Client::Gjc)
    );
}

#[test]
fn provider_and_model_identity_escape_separator_and_percent_without_collisions() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    for (id, provider, model) in [
        ("one", "a/b", "c"),
        ("two", "a", "b/c"),
        ("three", "a%2Fb", "c"),
        ("four", "a", "b%2Fc"),
        ("five", "other", "c"),
    ] {
        let mut data = assistant();
        data["providerID"] = json!(provider);
        data["modelID"] = json!(model);
        message(&db, id, "s", data);
    }
    let snapshot = aggregate(vec![sources::opencode::parse(&path).unwrap()]);
    let mut models = snapshot.sessions[0]
        .models
        .iter()
        .map(|m| m.model.as_str())
        .collect::<Vec<_>>();
    models.sort();
    assert_eq!(
        models,
        ["a%252Fb/c", "a%2Fb/c", "a/b%252Fc", "a/b%2Fc", "other/c"]
    );
    assert!(
        snapshot.sessions[0]
            .models
            .iter()
            .all(|m| m.record_count == 1 && m.tokens.total_tokens == 21)
    );
}

#[test]
fn malformed_json_does_not_prevent_later_valid_messages() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    raw_message(&db, "a-bad", "s", "{not json");
    message(&db, "z-valid", "s", assistant());
    let result = sources::opencode::parse(&path).unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].message_id.as_ref().unwrap().0, "z-valid");
    assert_eq!(result.records[0].tokens.total_tokens, 21);
    assert_eq!(result.summary.records_skipped, 1);
    assert_diagnostic_paths(&result, &path);
}

#[test]
fn missing_required_metadata_and_numeric_components_are_not_defaulted() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    for (index, pointer) in [
        "/modelID",
        "/providerID",
        "/tokens/input",
        "/tokens/output",
        "/tokens/reasoning",
        "/tokens/cache/read",
        "/tokens/cache/write",
    ]
    .iter()
    .enumerate()
    {
        let mut data = assistant();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        data.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        message(&db, &format!("missing-{index}"), "s", data);
    }
    message(&db, "valid", "s", assistant());
    let result = sources::opencode::parse(&path).unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.summary.records_skipped, 7);
    assert!(result.diagnostics.len() >= 7);
    assert_diagnostic_paths(&result, &path);
}

#[test]
fn invalid_numeric_components_skip_rows_without_poisoning_valid_usage() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    for (index, pointer) in [
        "/tokens/input",
        "/tokens/output",
        "/tokens/reasoning",
        "/tokens/cache/read",
        "/tokens/cache/write",
    ]
    .iter()
    .enumerate()
    {
        for (kind, invalid) in [
            ("negative", json!(-1)),
            ("fraction", json!(1.5)),
            ("string", json!("10")),
            ("null", Value::Null),
        ] {
            let mut data = assistant();
            *data.pointer_mut(pointer).unwrap() = invalid;
            message(&db, &format!("{index}-{kind}"), "s", data);
        }
    }
    let mut overflow = assistant();
    overflow["tokens"]["input"] = json!(u64::MAX);
    message(&db, "component-sum-overflow", "s", overflow);
    raw_message(
        &db,
        "above-u64",
        "s",
        r#"{"role":"assistant","providerID":"p","modelID":"m","time":{"created":0},"tokens":{"input":18446744073709551616,"output":0,"reasoning":0,"cache":{"read":0,"write":0}}}"#,
    );
    message(&db, "valid", "s", assistant());
    let result = sources::opencode::parse(&path).unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].tokens.total_tokens, 21);
    assert_eq!(result.summary.records_skipped, 22);
    assert!(result.diagnostics.len() >= 22);
    assert_diagnostic_paths(&result, &path);
}

#[test]
fn contradictory_reported_total_uses_canonical_components_with_diagnostic() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    let mut data = assistant();
    data["tokens"]["total"] = json!(999);
    message(&db, "mismatch", "s", data);
    let result = sources::opencode::parse(&path).unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].tokens.input_total, 14);
    assert_eq!(result.records[0].tokens.output_total, 7);
    assert_eq!(result.records[0].tokens.total_tokens, 21);
    assert_diagnostic_paths(&result, &path);
}

#[test]
fn zero_completed_by_finish_or_time_is_kept_but_placeholder_is_skipped() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    for id in ["finish", "completed", "placeholder"] {
        let mut data = assistant();
        data["tokens"] = json!({"input":0,"output":0,"reasoning":0,"cache":{"read":0,"write":0}});
        if id == "finish" {
            data["finish"] = json!("stop");
        }
        if id == "completed" {
            data["time"]["completed"] = json!(1);
        }
        message(&db, id, "s", data);
    }
    let result = sources::opencode::parse(&path).unwrap();
    let mut ids = result
        .records
        .iter()
        .map(|r| r.message_id.as_ref().unwrap().0.as_str())
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, ["completed", "finish"]);
    assert_eq!(result.summary.records_skipped, 1);
    assert!(result.records.iter().all(|r| r.tokens.total_tokens == 0));
}

#[test]
fn timestamps_missing_or_invalid_remain_unknown_then_use_common_filter() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    let mut missing = assistant();
    missing.as_object_mut().unwrap().remove("time");
    message(&db, "missing", "s", missing);
    for (id, timestamp) in [
        ("bad", json!("bad")),
        ("fraction", json!(1.5)),
        ("range", json!(i64::MAX)),
    ] {
        let mut data = assistant();
        data["time"]["created"] = timestamp;
        message(&db, id, "s", data);
    }
    let parsed = sources::opencode::parse(&path).unwrap();
    assert_eq!(parsed.records.len(), 4);
    assert!(parsed.records.iter().all(|r| r.started_at.is_none()));
    assert!(parsed.diagnostics.len() >= 4);
    assert_diagnostic_paths(&parsed, &path);
    assert_eq!(
        aggregate(vec![parsed.clone()]).sessions[0]
            .tokens
            .total_tokens,
        84
    );
    let snapshot = aggregate_with_options(
        vec![parsed],
        &AggregateOptions {
            time_selection: time::TimeSelection {
                since: Some(Utc.timestamp_millis_opt(0).unwrap()),
                until: None,
            },
            group_by: Some(GroupBy::Day),
        },
    )
    .unwrap();
    assert!(snapshot.sessions.is_empty());
    assert!(snapshot.timeline.is_empty());
    assert_eq!(snapshot.source_counts.records_missing_timestamp_filtered, 4);
}

#[test]
fn date_filter_retains_root_attribution_when_parent_message_is_outside_range() {
    let (_dir, path, db) = database(false);
    session(&db, "root", None);
    session(&db, "child", Some("root"));
    for (id, sid, day) in [("old", "root", 31), ("a", "child", 1), ("b", "child", 2)] {
        let month = if id == "old" { 8 } else { 9 };
        let ts = Local
            .with_ymd_and_hms(2026, month, day, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        let mut data = assistant();
        data["time"]["created"] = json!(ts);
        message(&db, id, sid, data);
    }
    let now = Local.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap();
    for (group, expected_starts) in [
        (GroupBy::Day, vec![(9, 1), (9, 2)]),
        (GroupBy::Week, vec![(8, 31)]),
        (GroupBy::Month, vec![(9, 1)]),
    ] {
        let snapshot = aggregate_with_options(
            vec![sources::opencode::parse(&path).unwrap()],
            &AggregateOptions {
                time_selection: time::build_selection(
                    false,
                    Some("2026-09-01"),
                    Some("2026-09-02"),
                    now,
                )
                .unwrap(),
                group_by: Some(group),
            },
        )
        .unwrap();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].key.id.0, "root");
        assert_eq!(snapshot.sessions[0].name.as_deref(), Some("root"));
        assert_eq!(snapshot.sessions[0].record_count, 2);
        assert_eq!(snapshot.sessions[0].tokens.total_tokens, 42);
        assert_eq!(snapshot.source_counts.records_time_filtered, 1);
        assert_eq!(
            snapshot
                .timeline
                .iter()
                .map(|b| b.tokens.total_tokens)
                .sum::<u64>(),
            42
        );
        let expected = expected_starts
            .into_iter()
            .map(|(month, day)| {
                Local
                    .with_ymd_and_hms(2026, month, day, 0, 0, 0)
                    .unwrap()
                    .with_timezone(&Utc)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            snapshot
                .timeline
                .iter()
                .map(|b| b.start)
                .collect::<Vec<_>>(),
            expected
        );
    }
    let today = aggregate_with_options(
        vec![sources::opencode::parse(&path).unwrap()],
        &AggregateOptions {
            time_selection: time::build_selection(
                true,
                None,
                None,
                Local.with_ymd_and_hms(2026, 9, 2, 12, 0, 0).unwrap(),
            )
            .unwrap(),
            group_by: Some(GroupBy::Day),
        },
    )
    .unwrap();
    assert_eq!(today.sessions[0].tokens.total_tokens, 21);
    assert_eq!(today.source_counts.records_time_filtered, 2);
}

#[test]
fn deep_chains_orphans_and_cycles_have_deterministic_roots() {
    let (_dir, path, db) = database(false);
    session(&db, "root", None);
    for index in 0..1024 {
        let parent = if index == 0 {
            "root".to_owned()
        } else {
            format!("deep-{}", index - 1)
        };
        session(&db, &format!("deep-{index}"), Some(&parent));
    }
    message(&db, "deep", "deep-1023", assistant());
    session(&db, "orphan", Some("absent"));
    session(&db, "orphan-child", Some("orphan"));
    message(&db, "orphan", "orphan-child", assistant());
    session(&db, "cycle-a", Some("cycle-b"));
    session(&db, "cycle-b", Some("cycle-a"));
    session(&db, "cycle-child", Some("cycle-a"));
    session(&db, "self", Some("self"));
    for sid in ["cycle-a", "cycle-b", "cycle-child", "self"] {
        message(&db, sid, sid, assistant());
    }
    let result = sources::opencode::parse(&path).unwrap();
    assert_eq!(result.records.len(), 6);
    for (id, expected) in [
        ("deep", "root"),
        ("orphan", "orphan"),
        ("cycle-a", "cycle-a"),
        ("cycle-b", "cycle-b"),
        ("cycle-child", "cycle-child"),
        ("self", "self"),
    ] {
        let record = result
            .records
            .iter()
            .find(|r| r.message_id.as_ref().unwrap().0 == id)
            .unwrap();
        assert_eq!(record.session_key.id.0, expected);
    }
    assert_diagnostic_paths(&result, &path);
    assert_eq!(aggregate(vec![result]).sessions.len(), 6);
}

#[test]
fn missing_database_is_a_warning_and_is_never_created() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("missing.db");
    let result = sources::opencode::parse(&path).unwrap();
    assert!(result.records.is_empty());
    assert!(!path.exists());
    assert_eq!(result.summary.missing_roots, vec![path.clone()]);
    assert!(result.diagnostics.iter().any(|d| d.code == "OpenCodeMissingDatabase" && matches!(d.severity, Severity::Warning)));
    assert_diagnostic_paths(&result, &path);
}

#[test]
fn invalid_database_and_missing_schema_return_diagnostics_without_partial_records() {
    let dir = tempdir().unwrap();
    let corrupt = dir.path().join("corrupt.db");
    fs::write(&corrupt, b"not a SQLite database").unwrap();
    let result = sources::opencode::parse(&corrupt).unwrap();
    assert!(result.records.is_empty());
    assert_diagnostic_paths(&result, &corrupt);
    for (index, schema) in [
        "CREATE TABLE unrelated(id TEXT);",
        "CREATE TABLE session(id TEXT PRIMARY KEY, parent_id TEXT, title TEXT); CREATE TABLE message(id TEXT PRIMARY KEY, session_id TEXT, data TEXT);",
        "CREATE TABLE session(id TEXT PRIMARY KEY, title TEXT); CREATE TABLE message(id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
    ].iter().enumerate() {
        let path = dir.path().join(format!("schema-{index}.db"));
        let db = Connection::open(&path).unwrap();
        db.execute_batch(schema).unwrap();
        drop(db);
        let result = sources::opencode::parse(&path).unwrap();
        assert!(result.records.is_empty());
        assert_eq!(result.summary.records_emitted, 0);
        assert_diagnostic_paths(&result, &path);
    }
}

#[test]
fn optional_part_table_is_not_required() {
    let (_dir, path, db) = database(false);
    db.execute_batch("DROP TABLE part;").unwrap();
    session(&db, "s", None);
    message(&db, "m", "s", assistant());
    let result = sources::opencode::parse(&path).unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].tokens.total_tokens, 21);
}

#[test]
fn parsing_is_read_only_and_refresh_replaces_updated_message_usage() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    message(&db, "m", "s", assistant());
    let before = fs::read(&path).unwrap();
    let first = sources::opencode::parse(&path).unwrap();
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].tokens.total_tokens, 21);
    assert_eq!(fs::read(&path).unwrap(), before);
    let mut updated = assistant();
    updated["tokens"]["input"] = json!(100);
    db.execute(
        "UPDATE message SET data = ?1 WHERE id = 'm'",
        [updated.to_string()],
    )
    .unwrap();
    let updated_bytes = fs::read(&path).unwrap();
    let second = sources::opencode::parse(&path).unwrap();
    assert_eq!(second.records.len(), 1);
    assert_eq!(second.records[0].tokens.total_tokens, 111);
    assert_eq!(aggregate(vec![second]).sessions[0].tokens.total_tokens, 111);
    assert_eq!(fs::read(&path).unwrap(), updated_bytes);
}

#[test]
fn live_wal_reader_sees_committed_usage_but_not_uncommitted_writes() {
    let (_dir, path, db) = database(false);
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    session(&db, "s", None);
    message(&db, "committed", "s", assistant());
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    assert!(fs::metadata(&wal).unwrap().len() > 0);
    let db_bytes = fs::read(&path).unwrap();
    let wal_bytes = fs::read(&wal).unwrap();
    db.execute_batch("BEGIN IMMEDIATE;").unwrap();
    message(&db, "uncommitted", "s", assistant());
    let first = sources::opencode::parse(&path).unwrap();
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].message_id.as_ref().unwrap().0, "committed");
    assert_eq!(first.records[0].tokens.total_tokens, 21);
    assert_eq!(fs::read(&path).unwrap(), db_bytes);
    assert_eq!(fs::read(&wal).unwrap(), wal_bytes);
    db.execute_batch("COMMIT;").unwrap();
    let second = sources::opencode::parse(&path).unwrap();
    assert_eq!(second.records.len(), 2);
    assert_eq!(aggregate(vec![second]).sessions[0].tokens.total_tokens, 42);
}

#[test]
fn cli_accepts_explicit_opencode_database_and_uses_no_other_source() {
    let (_dir, path, _db) = database(true);
    let parsed = cli::Cli::try_parse_from([
        "token-usage",
        "--client",
        "opencode",
        "--opencode-db",
        path.to_str().unwrap(),
        "--json",
    ])
    .unwrap();
    assert!(matches!(parsed.client, cli::ClientFilter::Opencode));
    assert_eq!(parsed.opencode_db.as_deref(), Some(path.as_path()));
    let snapshot = cli::build_snapshot(&parsed).unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.sessions[0].key.client, Client::OpenCode);
    assert_eq!(snapshot.sessions[0].tokens.total_tokens, 197);
    assert_eq!(snapshot.source_counts.roots_scanned, vec![path.clone()]);
    let mut command = assert_cmd::Command::cargo_bin("token-usage").unwrap();
    let output = command
        .args(["--client", "opencode", "--opencode-db"])
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(value["sessions"][0]["tokens"]["total_tokens"], json!(197));
}

fn max_token_message() -> Value {
    json!({"role":"assistant","providerID":"p","modelID":"m","time":{"created":0},
        "tokens":{"input":u64::MAX,"output":0,"reasoning":0,"cache":{"read":0,"write":0},"total":u64::MAX}})
}

#[test]
fn cli_json_preserves_valid_u64_max_without_signed_or_float_coercion() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    message(&db, "max", "s", max_token_message());
    let mut command = assert_cmd::Command::cargo_bin("token-usage").unwrap();
    let output = command
        .args(["--client", "opencode", "--opencode-db"])
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["sessions"][0]["tokens"]["total_tokens"].as_u64(),
        Some(u64::MAX)
    );
}

#[test]
fn cli_overflow_fails_instead_of_panicking_or_wrapping() {
    let (_dir, path, db) = database(false);
    session(&db, "s", None);
    message(&db, "max", "s", max_token_message());
    let mut one = max_token_message();
    one["tokens"]["input"] = json!(1);
    one["tokens"]["total"] = json!(1);
    message(&db, "one", "s", one);
    let parsed = sources::opencode::parse(&path).unwrap();
    assert_eq!(parsed.records.len(), 2);
    let mut command = assert_cmd::Command::cargo_bin("token-usage").unwrap();
    let output = command
        .args(["--client", "opencode", "--opencode-db"])
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.to_ascii_lowercase().contains("overflow"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn exclusive_lock_reports_database_failure_and_recovers_after_release() {
    let (_dir, path, db) = database(false);
    session(&db, "root", None);
    message(&db, "m", "root", assistant());
    db.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let locked = sources::opencode::parse(&path).unwrap();
    assert!(locked.records.is_empty());
    assert!(locked.summary.empty_roots.is_empty());
    assert!(
        locked
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, Severity::Error))
    );
    db.execute_batch("ROLLBACK;").unwrap();
    let recovered = sources::opencode::parse(&path).unwrap();
    assert_eq!(recovered.records.len(), 1);
    assert_eq!(recovered.records[0].tokens.total_tokens, 21);
}

#[cfg(unix)]
#[test]
fn rollback_database_can_be_read_without_file_or_directory_write_permission() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, path, db) = database(false);
    session(&db, "root", None);
    message(&db, "m", "root", assistant());
    drop(db);
    let before = fs::read(&path).unwrap();
    let file_permissions = fs::metadata(&path).unwrap().permissions();
    let dir_permissions = fs::metadata(dir.path()).unwrap().permissions();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let result = sources::opencode::parse(&path);
    fs::set_permissions(dir.path(), dir_permissions).unwrap();
    fs::set_permissions(&path, file_permissions).unwrap();
    let parsed = result.unwrap();
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].tokens.total_tokens, 21);
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn invalid_roles_are_diagnosed_while_user_messages_are_normally_excluded() {
    let (_dir, path, db) = database(false);
    session(&db, "root", None);
    let mut missing = assistant();
    missing.as_object_mut().unwrap().remove("role");
    message(&db, "a-missing", "root", missing);
    for (id, role) in [
        ("b-null", Value::Null),
        ("c-number", json!(42)),
        ("d-object", json!({"role": "assistant"})),
        ("e-unknown", json!("tool")),
        ("f-user", json!("user")),
    ] {
        let mut data = assistant();
        data["role"] = role;
        message(&db, id, "root", data);
    }
    message(&db, "z-valid", "root", assistant());
    let parsed = sources::opencode::parse(&path).unwrap();
    assert_eq!(parsed.summary.records_emitted, 1);
    assert_eq!(parsed.summary.records_skipped, 5);
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].message_id.as_ref().unwrap().0, "z-valid");
    assert_eq!(parsed.records[0].tokens.total_tokens, 21);
    assert_eq!(parsed.diagnostics.len(), 5);
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|d| d.code == "OpenCodeInvalidRole")
    );
    assert_diagnostic_paths(&parsed, &path);
}

#[test]
fn missing_own_session_keeps_original_identity_and_diagnoses_once_per_session() {
    let (_dir, path, db) = database(false);
    session(&db, "root", None);
    message(&db, "a-absent", "absent", assistant());
    message(&db, "b-absent", "absent", assistant());
    message(&db, "c-another", "another-absent", assistant());
    message(&db, "d-root", "root", assistant());
    let parsed = sources::opencode::parse(&path).unwrap();
    assert_eq!(parsed.summary.records_emitted, 4);
    assert_eq!(parsed.summary.records_skipped, 0);
    assert_eq!(parsed.records.len(), 4);
    for record in &parsed.records {
        assert_eq!(record.tokens.total_tokens, 21);
        assert_eq!(record.session_key.client, Client::OpenCode);
        assert!(record.parent_session_key.is_none());
        match record.message_id.as_ref().unwrap().0.as_str() {
            "a-absent" | "b-absent" => {
                assert_eq!(record.session_key.id.0, "absent");
                assert!(record.session_name.is_none());
            }
            "c-another" => {
                assert_eq!(record.session_key.id.0, "another-absent");
                assert!(record.session_name.is_none());
            }
            "d-root" => {
                assert_eq!(record.session_key.id.0, "root");
                assert_eq!(record.session_name.as_deref(), Some("root"));
            }
            other => panic!("unexpected message: {other}"),
        }
    }
    assert_eq!(parsed.diagnostics.len(), 2);
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|d| d.code == "OpenCodeMissingSession")
    );
    assert_diagnostic_paths(&parsed, &path);
    let snapshot = aggregate(vec![parsed]);
    assert_eq!(snapshot.sessions.len(), 3);
    let absent = resolve_session(&snapshot, "opencode:absent").unwrap();
    assert_eq!(absent.record_count, 2);
    assert_eq!(absent.tokens.total_tokens, 42);
    assert!(absent.name.is_none());
    assert!(absent.parent.is_none());
    for id in ["opencode:another-absent", "opencode:root"] {
        let session = resolve_session(&snapshot, id).unwrap();
        assert_eq!(session.record_count, 1);
        assert_eq!(session.tokens.total_tokens, 21);
    }
}

fn boundary_messages(db: &Connection, start: i64, end: i64) {
    session(db, "root", None);
    session(db, "child", Some("root"));
    for (id, sid, timestamp) in [
        ("before-since", "root", start - 1),
        ("at-since", "child", start),
        ("after-since", "child", start + 1),
        ("before-until", "child", end - 1),
        ("at-until", "root", end),
        ("after-until", "root", end + 1),
    ] {
        let mut data = assistant();
        data["time"]["created"] = json!(timestamp);
        message(db, id, sid, data);
    }
}

#[test]
fn source_time_selection_and_buckets_honor_exact_local_midnight_boundaries() {
    let (_dir, path, db) = database(false);
    let start = Local
        .with_ymd_and_hms(2026, 9, 2, 0, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let end = Local
        .with_ymd_and_hms(2026, 9, 3, 0, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    boundary_messages(&db, start.timestamp_millis(), end.timestamp_millis());
    let parsed = sources::opencode::parse(&path).unwrap();
    assert_eq!(parsed.records.len(), 6);
    assert!(parsed.diagnostics.is_empty());
    let selection = time::build_selection(
        false,
        Some("2026-09-02"),
        Some("2026-09-02"),
        Local.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap(),
    )
    .unwrap();
    assert_eq!(selection.since, Some(start));
    assert_eq!(selection.until, Some(end));
    for (group, expected_start) in [
        (GroupBy::Day, start),
        (
            GroupBy::Week,
            Local
                .with_ymd_and_hms(2026, 8, 31, 0, 0, 0)
                .unwrap()
                .with_timezone(&Utc),
        ),
        (
            GroupBy::Month,
            Local
                .with_ymd_and_hms(2026, 9, 1, 0, 0, 0)
                .unwrap()
                .with_timezone(&Utc),
        ),
    ] {
        let snapshot = aggregate_with_options(
            vec![parsed.clone()],
            &AggregateOptions {
                time_selection: selection.clone(),
                group_by: Some(group),
            },
        )
        .unwrap();
        assert_eq!(snapshot.source_counts.records_time_filtered, 3);
        assert_eq!(snapshot.source_counts.records_missing_timestamp_filtered, 0);
        assert_eq!(snapshot.sessions.len(), 1);
        let root = &snapshot.sessions[0];
        assert_eq!(root.key.id.0, "root");
        assert_eq!(root.name.as_deref(), Some("root"));
        assert_eq!(root.record_count, 3);
        assert_eq!(root.tokens.total_tokens, 63);
        assert_eq!(root.started_at, Some(start));
        assert_eq!(snapshot.timeline.len(), 1);
        assert_eq!(snapshot.timeline[0].start, expected_start);
        assert_eq!(snapshot.timeline[0].record_count, 3);
        assert_eq!(snapshot.timeline[0].tokens.total_tokens, 63);
    }
}

fn utc_millis(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}

fn assert_new_york_transition(
    date: &str,
    start: &str,
    end: &str,
    transition_times: [&str; 2],
    week_start: &str,
    month_start: &str,
    expected_hours: i64,
) {
    let (_dir, path, db) = database(false);
    let start_ms = utc_millis(start);
    let end_ms = utc_millis(end);
    assert_eq!(end_ms - start_ms, expected_hours * 60 * 60 * 1000);
    boundary_messages(&db, start_ms, end_ms);
    for (index, timestamp) in transition_times.into_iter().enumerate() {
        let mut data = assistant();
        data["time"]["created"] = json!(utc_millis(timestamp));
        message(&db, &format!("transition-{index}"), "child", data);
    }
    drop(db);
    for (group, bucket_start) in [("day", start), ("week", week_start), ("month", month_start)] {
        let mut command = assert_cmd::Command::cargo_bin("token-usage").unwrap();
        let output = command
            .env("TZ", "America/New_York")
            .args(["--client", "opencode", "--opencode-db"])
            .arg(&path)
            .args([
                "--since",
                date,
                "--until",
                date,
                "--group-by",
                group,
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["source_counts"]["records_emitted"], json!(8));
        assert_eq!(value["source_counts"]["records_time_filtered"], json!(3));
        assert_eq!(
            value["source_counts"]["records_missing_timestamp_filtered"],
            json!(0)
        );
        assert_eq!(value["sessions"].as_array().unwrap().len(), 1);
        let root = &value["sessions"][0];
        assert_eq!(root["key"]["id"], json!("root"));
        assert_eq!(root["name"], json!("root"));
        assert_eq!(root["record_count"], json!(5));
        assert_eq!(root["tokens"]["total_tokens"], json!(105));
        assert_eq!(utc_millis(root["started_at"].as_str().unwrap()), start_ms);
        assert_eq!(value["timeline"].as_array().unwrap().len(), 1);
        let bucket = &value["timeline"][0];
        assert_eq!(
            utc_millis(bucket["start"].as_str().unwrap()),
            utc_millis(bucket_start)
        );
        assert_eq!(bucket["record_count"], json!(5));
        assert_eq!(bucket["tokens"]["total_tokens"], json!(105));
        assert!(value["diagnostics"].as_array().unwrap().is_empty());
    }
}

#[test]
fn new_york_spring_forward_source_day_is_23_hours_with_exact_buckets() {
    assert_new_york_transition(
        "2026-03-08",
        "2026-03-08T05:00:00Z",
        "2026-03-09T04:00:00Z",
        ["2026-03-08T06:59:59.999Z", "2026-03-08T07:00:00Z"],
        "2026-03-02T05:00:00Z",
        "2026-03-01T05:00:00Z",
        23,
    );
}

#[test]
fn new_york_fall_back_source_day_includes_both_repeated_hours_and_is_25_hours() {
    assert_new_york_transition(
        "2026-11-01",
        "2026-11-01T04:00:00Z",
        "2026-11-02T05:00:00Z",
        ["2026-11-01T05:30:00Z", "2026-11-01T06:30:00Z"],
        "2026-10-26T04:00:00Z",
        "2026-11-01T04:00:00Z",
        25,
    );
}
