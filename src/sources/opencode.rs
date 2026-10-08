use crate::{
    diagnostics::Diagnostic,
    domain::{Client, MessageId, SessionId, SessionKey, TokenStats, UsageRecord},
    scan::{ParseResult, ScanSummary},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn default_db_path() -> Result<PathBuf> {
    let override_path = env::var_os("OPENCODE_DB").filter(|v| !v.is_empty());
    if let Some(value) = &override_path {
        let path = PathBuf::from(value);
        reject_memory(&path)?;
        if path.is_absolute() {
            return Ok(path);
        }
    }
    let data_dir = match env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => PathBuf::from(
            env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .context("OpenCode default database path requires XDG_DATA_HOME or HOME")?,
        )
        .join(".local/share"),
    };
    Ok(match override_path {
        Some(value) => data_dir.join("opencode").join(value),
        None => data_dir.join("opencode/opencode.db"),
    })
}

fn reject_memory(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        bail!("OpenCode requires a database file, not :memory:");
    }
    Ok(())
}

fn open_readonly(path: &Path) -> Result<Connection> {
    reject_memory(path)?;
    // Deliberately omit URI, CREATE and READ_WRITE; no writable retry is allowed.
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open OpenCode database {} read-only", path.display()))?;
    connection.busy_timeout(Duration::from_secs(2))?;
    connection.pragma_update(None, "query_only", true)?;
    if !connection.is_readonly("main")? {
        bail!("OpenCode database connection is not read-only");
    }
    Ok(connection)
}

fn warning(path: &Path, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::warning(
        Client::OpenCode,
        code,
        message,
        Some(path.to_path_buf()),
        None,
    )
}

pub fn parse(path: &Path) -> Result<ParseResult> {
    reject_memory(path)?;
    let mut result = ParseResult {
        records: Vec::new(),
        diagnostics: Vec::new(),
        summary: ScanSummary {
            client: Some(Client::OpenCode),
            ..Default::default()
        },
    };
    match path.try_exists() {
        Ok(false) => {
            result.summary.missing_roots.push(path.to_path_buf());
            result.diagnostics.push(warning(
                path,
                "OpenCodeMissingDatabase",
                "OpenCode database is missing",
            ));
            return Ok(result);
        }
        Ok(true) => {}
        Err(error) => {
            result.diagnostics.push(Diagnostic::error(
                Client::OpenCode,
                "OpenCodeDatabaseError",
                error.to_string(),
                Some(path.to_path_buf()),
                None,
            ));
            return Ok(result);
        }
    }
    result.summary.roots_scanned.push(path.to_path_buf());
    if let Err(error) = read_database(path, &mut result) {
        // A failed snapshot is not a valid partial usage report.
        result.records.clear();
        result.summary.records_emitted = 0;
        result.diagnostics.push(Diagnostic::error(
            Client::OpenCode,
            "OpenCodeDatabaseError",
            format!("{error:#}"),
            Some(path.to_path_buf()),
            None,
        ));
    } else if result.records.is_empty() {
        result.summary.empty_roots.push(path.to_path_buf());
    }
    Ok(result)
}

#[derive(Debug)]
struct Session {
    parent: Option<String>,
    title: Option<String>,
}

// None means this node reaches a cycle: keep each original session's identity.
fn resolve_roots(
    sessions: &HashMap<String, Session>,
    path: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> HashMap<String, Option<String>> {
    let mut memo: HashMap<String, Option<String>> = HashMap::new();
    for start in sessions.keys() {
        if memo.contains_key(start) {
            continue;
        }
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = start.clone();
        let root = loop {
            if let Some(root) = memo.get(&current) {
                break root.clone();
            }
            if !visited.insert(current.clone()) {
                diagnostics.push(warning(path, "OpenCodeSessionCycle",
                    format!("session ancestry contains a cycle at {current}; affected messages retain their original session")));
                break None;
            }
            chain.push(current.clone());
            let session = &sessions[&current];
            match &session.parent {
                None => break Some(current),
                Some(parent) if sessions.contains_key(parent) => current = parent.clone(),
                Some(parent) => {
                    diagnostics.push(warning(path, "OpenCodeOrphanSession",
                        format!("session {current} has missing parent {parent}; using last existing ancestor")));
                    break Some(current);
                }
            }
        };
        for id in chain {
            memo.insert(id, root.clone());
        }
    }
    memo
}

fn read_database(path: &Path, result: &mut ParseResult) -> Result<()> {
    let mut connection = open_readonly(path)?;
    result.summary.files_scanned = 1;
    let transaction = connection.transaction()?;
    // Preparing these projections validates the required columns without loading rows.
    transaction
        .prepare("SELECT id, parent_id, title FROM session LIMIT 0")
        .context("unsupported OpenCode session schema")?;
    transaction
        .prepare("SELECT id, session_id, time_created, data FROM message LIMIT 0")
        .context("unsupported OpenCode message schema")?;
    let mut sessions = HashMap::new();
    {
        let mut statement = transaction.prepare("SELECT id, parent_id, title FROM session")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let metadata = (|| -> rusqlite::Result<(String, Session)> {
                Ok((
                    row.get(0)?,
                    Session {
                        parent: row.get(1)?,
                        title: row.get(2)?,
                    },
                ))
            })();
            match metadata {
                Ok((id, session)) => {
                    sessions.insert(id, session);
                }
                Err(error) => result.diagnostics.push(warning(
                    path,
                    "OpenCodeInvalidSession",
                    error.to_string(),
                )),
            }
        }
    }
    let roots = resolve_roots(&sessions, path, &mut result.diagnostics);
    let mut missing_sessions = HashSet::new();
    // -> returns the original JSON numeric text, not SQLite's signed i64/REAL
    // coercion. Every JSON operation is behind json_valid, and only scalars
    // cross into Rust (never message.data, token objects, or parts).
    let numeric_paths = [
        "$.time.created",
        "$.tokens.input",
        "$.tokens.output",
        "$.tokens.reasoning",
        "$.tokens.cache.read",
        "$.tokens.cache.write",
    ];
    let mut sql = String::from(
        "SELECT id, session_id, json_valid(data), \
        CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.role') = 'text' THEN json_extract(data, '$.role') END END, \
        CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.providerID') = 'text' THEN json_extract(data, '$.providerID') END END, \
        CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '$.modelID') = 'text' THEN json_extract(data, '$.modelID') END END",
    );
    for field in numeric_paths {
        sql.push_str(&format!(", CASE WHEN json_valid(data) THEN CASE WHEN json_type(data, '{field}') IN ('integer', 'real') THEN data -> '{field}' END END"));
    }
    sql.push_str(", CASE WHEN json_valid(data) THEN CASE \
        WHEN json_type(data, '$.tokens.total') IN ('integer', 'real') THEN data -> '$.tokens.total' \
        WHEN json_type(data, '$.tokens.total') != 'null' THEN 'null' END END, \
        CASE WHEN json_valid(data) THEN \
        (json_type(data, '$.time.completed') IN ('integer', 'real') OR \
         COALESCE(json_type(data, '$.finish') != 'null', 0) OR \
         COALESCE(json_type(data, '$.error') != 'null', 0)) ELSE 0 END FROM message");
    {
        let mut statement = transaction.prepare(&sql)?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let projected = (|| -> rusqlite::Result<ProjectedMessage> {
                Ok(ProjectedMessage {
                    id: row.get(0)?,
                    session: row.get(1)?,
                    valid: row.get(2)?,
                    role: row.get(3)?,
                    provider: row.get(4)?,
                    model: row.get(5)?,
                    created: row.get(6)?,
                    usage: [
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                    ],
                    total: row.get(12)?,
                    finished: row.get::<_, Option<bool>>(13)?.unwrap_or(false),
                })
            })();
            let message = match projected {
                Ok(message) => message,
                Err(error) => {
                    result.summary.records_skipped += 1;
                    result.diagnostics.push(warning(
                        path,
                        "OpenCodeInvalidMessage",
                        error.to_string(),
                    ));
                    continue;
                }
            };
            if !message.valid {
                result.summary.records_skipped += 1;
                result.diagnostics.push(warning(
                    path,
                    "OpenCodeMalformedMessage",
                    format!("message {} has invalid JSON", message.id),
                ));
                continue;
            }
            match message.role.as_deref() {
                Some("assistant") => {}
                Some("user") => continue,
                _ => {
                    result.summary.records_skipped += 1;
                    result.diagnostics.push(warning(
                        path,
                        "OpenCodeInvalidRole",
                        format!(
                            "message {} has missing or unsupported role metadata",
                            message.id
                        ),
                    ));
                    continue;
                }
            }
            let record = make_record(
                path,
                message,
                &sessions,
                &roots,
                &mut missing_sessions,
                &mut result.diagnostics,
            );
            match record {
                Ok(record) => {
                    result.records.push(record);
                    result.summary.records_emitted += 1;
                }
                Err(error) => {
                    result.summary.records_skipped += 1;
                    result.diagnostics.push(warning(
                        path,
                        "OpenCodeMissingUsage",
                        format!("{error:#}"),
                    ));
                }
            }
        }
    }
    transaction.commit()?;
    Ok(())
}

struct ProjectedMessage {
    id: String,
    session: String,
    valid: bool,
    role: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    created: Option<String>,
    usage: [Option<String>; 5],
    total: Option<String>,
    finished: bool,
}

fn number(raw: Option<&str>) -> Result<u64> {
    let value: Value = serde_json::from_str(raw.context("missing or nonnumeric token component")?)?;
    value
        .as_u64()
        .context("token component is not a nonnegative u64 integer")
}

fn normalize_usage(usage: &[Option<String>; 5]) -> Result<TokenStats> {
    let input = number(usage[0].as_deref())?;
    let output = number(usage[1].as_deref())?;
    let reasoning = number(usage[2].as_deref())?;
    let cache_read = number(usage[3].as_deref())?;
    let cache_write = number(usage[4].as_deref())?;
    let input_total = input
        .checked_add(cache_read)
        .and_then(|n| n.checked_add(cache_write))
        .context("OpenCode input token sum overflow")?;
    let output_total = output
        .checked_add(reasoning)
        .context("OpenCode output token sum overflow")?;
    let total_tokens = input_total
        .checked_add(output_total)
        .context("OpenCode total token sum overflow")?;
    Ok(TokenStats {
        input_uncached: input,
        input_total,
        cache_read,
        cache_write,
        output_total,
        reasoning_known: reasoning,
        reasoning_has_unknown: false,
        total_tokens,
    })
}

fn escape_component(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

fn make_record(
    path: &Path,
    message: ProjectedMessage,
    sessions: &HashMap<String, Session>,
    roots: &HashMap<String, Option<String>>,
    missing_sessions: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<UsageRecord> {
    let model = message
        .model
        .as_deref()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("message {} has no modelID", message.id))?;
    let provider = message
        .provider
        .as_deref()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("message {} has no providerID", message.id))?;
    let tokens = normalize_usage(&message.usage)
        .with_context(|| format!("message {} usage skipped", message.id))?;
    if tokens.total_tokens == 0 && !message.finished {
        bail!(
            "message {} has initialized zero usage without completion, finish or error",
            message.id
        );
    }
    if let Some(raw) = message.total.as_deref() {
        match number(Some(raw)) {
            Ok(total) if total == tokens.total_tokens => {}
            Ok(_) => diagnostics.push(warning(
                path,
                "OpenCodeTotalMismatch",
                format!(
                    "message {} stored total differs from canonical components",
                    message.id
                ),
            )),
            Err(_) => diagnostics.push(warning(
                path,
                "OpenCodeInvalidTotal",
                format!(
                    "message {} has invalid optional total; canonical components retained",
                    message.id
                ),
            )),
        }
    }
    let started_at = message
        .created
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.as_i64())
        .and_then(DateTime::<Utc>::from_timestamp_millis);
    if started_at.is_none() {
        diagnostics.push(warning(
            path,
            "OpenCodeInvalidTimestamp",
            format!(
                "message {} has missing or invalid time.created; timestamp remains unknown",
                message.id
            ),
        ));
    }
    let root = roots
        .get(&message.session)
        .and_then(|root| root.as_ref())
        .unwrap_or(&message.session);
    if !sessions.contains_key(&message.session) && missing_sessions.insert(message.session.clone())
    {
        diagnostics.push(warning(
            path,
            "OpenCodeMissingSession",
            format!(
                "session {} is missing; retaining original message session",
                message.session
            ),
        ));
    }
    Ok(UsageRecord {
        session_key: SessionKey {
            client: Client::OpenCode,
            id: SessionId(root.clone()),
        },
        parent_session_key: None,
        message_id: Some(MessageId(message.id)),
        source_path: path.to_path_buf(),
        source_line: None,
        started_at,
        session_name: sessions.get(root).and_then(|s| s.title.clone()),
        model: format!("{}/{}", escape_component(provider), escape_component(model)),
        reasoning_effort: None,
        tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(values: [&str; 5]) -> [Option<String>; 5] {
        values.map(|value| Some(value.to_string()))
    }

    #[test]
    fn normalization_components_and_boundaries() {
        let tokens = normalize_usage(&usage(["10", "20", "3", "4", "5"])).unwrap();
        assert_eq!(
            (
                tokens.input_uncached,
                tokens.input_total,
                tokens.output_total,
                tokens.total_tokens
            ),
            (10, 19, 23, 42)
        );
        assert_eq!(tokens.reasoning_known, 3);
        assert_eq!(
            normalize_usage(&usage(["18446744073709551615", "0", "0", "0", "0"]))
                .unwrap()
                .total_tokens,
            u64::MAX
        );
        for invalid in ["-1", "1.5", "1.0", "18446744073709551616", "null", "\"1\""] {
            assert!(normalize_usage(&usage([invalid, "0", "0", "0", "0"])).is_err());
        }
        assert!(normalize_usage(&usage(["18446744073709551615", "0", "0", "1", "0"])).is_err());
        assert!(normalize_usage(&usage(["0", "18446744073709551615", "1", "0", "0"])).is_err());
        assert!(normalize_usage(&usage(["1", "18446744073709551615", "0", "0", "0"])).is_err());
        let mut missing = usage(["1", "2", "0", "0", "0"]);
        missing[2] = None;
        assert!(normalize_usage(&missing).is_err());
    }

    #[test]
    fn readonly_opener_cannot_write_even_without_query_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("usage.db");
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch("CREATE TABLE sample(value INTEGER); INSERT INTO sample VALUES (1);")
            .unwrap();
        let reader = open_readonly(&path).unwrap();
        assert!(reader.is_readonly("main").unwrap());
        assert!(
            reader
                .execute_batch("CREATE TABLE forbidden(value INTEGER)")
                .is_err()
        );
        reader.pragma_update(None, "query_only", false).unwrap();
        assert!(reader.execute("INSERT INTO sample VALUES (2)", []).is_err());
        assert_eq!(
            reader
                .query_row("SELECT value FROM sample", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let missing = directory.path().join("missing.db");
        assert!(open_readonly(&missing).is_err());
        assert!(!missing.exists());
        assert!(open_readonly(Path::new(":memory:")).is_err());
    }

    #[test]
    fn readonly_wal_sees_commits_but_keeps_a_consistent_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wal.db");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE sample(value INTEGER); INSERT INTO sample VALUES (1);").unwrap();
        let mut reader = open_readonly(&path).unwrap();
        let transaction = reader.transaction().unwrap();
        assert_eq!(
            transaction
                .query_row("SELECT COUNT(*) FROM sample", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        writer.execute("INSERT INTO sample VALUES (2)", []).unwrap();
        assert_eq!(
            transaction
                .query_row("SELECT COUNT(*) FROM sample", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        transaction.commit().unwrap();
        assert_eq!(
            reader
                .query_row("SELECT COUNT(*) FROM sample", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        reader.pragma_update(None, "query_only", false).unwrap();
        assert!(reader.execute_batch("DROP TABLE sample").is_err());
    }

    #[test]
    fn scalar_projection_preserves_u64_and_persisted_interrupted_usage() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("messages.db");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("CREATE TABLE session(id TEXT PRIMARY KEY, parent_id TEXT, title TEXT); \
            CREATE TABLE message(id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT); \
            INSERT INTO session VALUES ('root', NULL, 'Root title'), ('child', 'root', 'Child title');").unwrap();
        let cases = [
            (
                "max",
                r#"{"role":"assistant","providerID":"p/%","modelID":"m/%","time":{"created":1700000000000},"tokens":{"input":18446744073709551615,"output":0,"reasoning":0,"cache":{"read":0,"write":0}}}"#,
            ),
            (
                "interrupted",
                r#"{"role":"assistant","providerID":"p","modelID":"m","tokens":{"input":10,"output":20,"reasoning":3,"cache":{"read":4,"write":5},"total":1}}"#,
            ),
            (
                "placeholder",
                r#"{"role":"assistant","providerID":"p","modelID":"m","tokens":{"input":0,"output":0,"reasoning":0,"cache":{"read":0,"write":0}}}"#,
            ),
            (
                "missing-reasoning",
                r#"{"role":"assistant","providerID":"p","modelID":"m","tokens":{"input":1,"output":2,"cache":{"read":0,"write":0}}}"#,
            ),
            (
                "fraction",
                r#"{"role":"assistant","providerID":"p","modelID":"m","tokens":{"input":1.5,"output":0,"reasoning":0,"cache":{"read":0,"write":0}}}"#,
            ),
            (
                "too-large",
                r#"{"role":"assistant","providerID":"p","modelID":"m","tokens":{"input":18446744073709551616,"output":0,"reasoning":0,"cache":{"read":0,"write":0}}}"#,
            ),
            ("malformed", "{"),
            ("user", r#"{"role":"user"}"#),
        ];
        for (id, data) in cases {
            writer
                .execute(
                    "INSERT INTO message VALUES (?1, 'child', 123, ?2)",
                    (id, data),
                )
                .unwrap();
        }
        let result = parse(&path).unwrap();
        assert_eq!(
            (
                result.summary.files_scanned,
                result.summary.lines_read,
                result.summary.records_emitted,
                result.summary.records_skipped
            ),
            (1, 0, 2, 5)
        );
        let maximum = result
            .records
            .iter()
            .find(|r| r.message_id.as_ref().unwrap().0 == "max")
            .unwrap();
        assert_eq!(maximum.tokens.total_tokens, u64::MAX);
        assert_eq!(maximum.model, "p%2F%25/m%2F%25");
        assert_eq!(maximum.session_key.id.0, "root");
        assert_eq!(maximum.session_name.as_deref(), Some("Root title"));
        assert_eq!(
            maximum.started_at.unwrap().timestamp_millis(),
            1700000000000
        );
        assert!(maximum.parent_session_key.is_none());
        assert!(maximum.source_line.is_none());
        let interrupted = result
            .records
            .iter()
            .find(|r| r.message_id.as_ref().unwrap().0 == "interrupted")
            .unwrap();
        assert_eq!(interrupted.tokens.total_tokens, 42);
        assert!(interrupted.started_at.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "OpenCodeTotalMismatch")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "OpenCodeInvalidTimestamp")
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| matches!(d.severity, crate::diagnostics::Severity::Error))
        );
    }

    #[test]
    fn missing_empty_and_database_failure_are_distinct() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing.db");
        let missing = parse(&path).unwrap();
        assert_eq!(missing.summary.missing_roots, vec![path.clone()]);
        assert_eq!(missing.diagnostics[0].code, "OpenCodeMissingDatabase");
        assert!(!path.exists());
        let writer = Connection::open(&path).unwrap();
        let unsupported = parse(&path).unwrap();
        assert!(unsupported.records.is_empty());
        assert!(unsupported.summary.empty_roots.is_empty());
        assert!(
            unsupported
                .diagnostics
                .iter()
                .any(|d| d.code == "OpenCodeDatabaseError")
        );
        writer.execute_batch("CREATE TABLE session(id TEXT PRIMARY KEY, parent_id TEXT, title TEXT); \
            CREATE TABLE message(id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);").unwrap();
        let empty = parse(&path).unwrap();
        assert_eq!(empty.summary.empty_roots, vec![path]);
        assert!(empty.diagnostics.is_empty());
    }

    #[test]
    fn deep_ancestry_is_iterative_without_a_depth_cutoff() {
        let mut sessions = HashMap::new();
        sessions.insert(
            "0".into(),
            Session {
                parent: None,
                title: None,
            },
        );
        for id in 1..10_000 {
            sessions.insert(
                id.to_string(),
                Session {
                    parent: Some((id - 1).to_string()),
                    title: None,
                },
            );
        }
        let mut diagnostics = Vec::new();
        let roots = resolve_roots(&sessions, Path::new("fixture.db"), &mut diagnostics);
        assert_eq!(roots["9999"].as_deref(), Some("0"));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ancestry_keeps_orphans_and_cycle_entries_without_fake_roots() {
        let mut sessions = HashMap::new();
        for (id, parent) in [
            ("root", None),
            ("child", Some("root")),
            ("leaf", Some("child")),
            ("orphan", Some("absent")),
            ("orphan-child", Some("orphan")),
            ("a", Some("b")),
            ("b", Some("a")),
            ("entry", Some("a")),
            ("self", Some("self")),
        ] {
            sessions.insert(
                id.to_string(),
                Session {
                    parent: parent.map(str::to_string),
                    title: None,
                },
            );
        }
        let mut diagnostics = Vec::new();
        let roots = resolve_roots(&sessions, Path::new("fixture.db"), &mut diagnostics);
        assert_eq!(roots["leaf"].as_deref(), Some("root"));
        assert_eq!(roots["orphan-child"].as_deref(), Some("orphan"));
        for id in ["a", "b", "entry", "self"] {
            assert_eq!(roots[id], None);
        }
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == "OpenCodeSessionCycle")
                .count(),
            2
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.code == "OpenCodeOrphanSession")
                .count(),
            1
        );
    }
}
