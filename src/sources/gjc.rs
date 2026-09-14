use crate::{
    diagnostics::Diagnostic,
    domain::{Client, MessageId, ReasoningEffort, SessionId, SessionKey, UsageRecord},
    scan::{ParseResult, ScanSummary},
    sources::codex::{string_at, token_stats, u64_at},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};
use walkdir::WalkDir;

pub fn parse(root: &Path) -> Result<ParseResult> {
    let mut summary = ScanSummary {
        client: Some(Client::Gjc),
        ..Default::default()
    };
    if !root.exists() {
        summary.missing_roots.push(root.to_path_buf());
        return Ok(ParseResult {
            records: vec![],
            diagnostics: vec![],
            summary,
        });
    }
    summary.roots_scanned.push(root.to_path_buf());
    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    let mut files = 0;
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        if entry.path().extension().and_then(|v| v.to_str()) != Some("jsonl") {
            continue;
        }
        files += 1;
        parse_file(entry.path(), &mut records, &mut diagnostics, &mut summary)?;
    }
    if files == 0 {
        summary.empty_roots.push(root.to_path_buf());
    }
    Ok(ParseResult {
        records,
        diagnostics,
        summary,
    })
}

fn parse_file(
    path: &Path,
    records: &mut Vec<UsageRecord>,
    diagnostics: &mut Vec<Diagnostic>,
    summary: &mut ScanSummary,
) -> Result<()> {
    summary.files_scanned += 1;
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let fallback_id = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut session_id = fallback_id;
    let mut parent_id = None::<String>;
    let mut title = None::<String>;
    let mut branch_effort: HashMap<String, Option<ReasoningEffort>> = HashMap::new();
    let mut branch_parent: HashMap<String, String> = HashMap::new();
    let mut seen = HashSet::new();

    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line_no = idx as u64 + 1;
        summary.lines_read += 1;
        let line = line?;
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                summary.records_skipped += 1;
                diagnostics.push(Diagnostic::warning(
                    Client::Gjc,
                    "GjcMalformedLine",
                    e.to_string(),
                    Some(path.to_path_buf()),
                    Some(line_no),
                ));
                continue;
            }
        };
        let typ = string_at(&v, &["type", "event"]);
        if matches!(
            typ.as_deref(),
            Some("header") | Some("session") | Some("header_patch")
        ) {
            if let Some(id) = string_at(&v, &["session_id", "sessionId", "id"]) {
                session_id = id;
            }
            if let Some(p) = string_at(
                &v,
                &[
                    "parent_session_id",
                    "parentSessionId",
                    "parent_id",
                    "parentId",
                ],
            ) {
                parent_id = Some(p);
            }
            if let Some(t) = string_at(&v, &["title", "name"]) {
                title = Some(t);
            }
        }
        if matches!(typ.as_deref(), Some("thinking_level_change")) {
            let branch = string_at(&v, &["branch", "branch_id", "branchId"])
                .unwrap_or_else(|| "main".into());
            if let Some(parent) = string_at(
                &v,
                &[
                    "parent_branch",
                    "parentBranch",
                    "parent_branch_id",
                    "parentBranchId",
                ],
            ) {
                branch_parent.insert(branch.clone(), parent);
            }
            if let Some(e) = string_at(&v, &["effort", "thinking_level", "thinkingLevel", "level"])
                && e != "inherit"
            {
                branch_effort.insert(branch, ReasoningEffort::parse(&e));
            }
            continue;
        }
        let message = v.get("message").unwrap_or(&v);
        let role = string_at(message, &["role"]).or_else(|| string_at(&v, &["role"]));
        if role.as_deref() != Some("assistant") && typ.as_deref() != Some("assistant") {
            continue;
        }
        let Some(usage) = message.get("usage").or_else(|| v.get("usage")) else {
            continue;
        };
        let Some(model) = string_at(message, &["model"]).or_else(|| string_at(&v, &["model"]))
        else {
            summary.records_skipped += 1;
            diagnostics.push(Diagnostic::warning(
                Client::Gjc,
                "GjcMissingModel",
                "assistant usage skipped because model is missing",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
            continue;
        };
        let msg_id = string_at(message, &["id", "message_id", "messageId"])
            .or_else(|| string_at(&v, &["message_id", "messageId", "id"]));
        let key = msg_id.clone().unwrap_or_else(|| {
            format!(
                "{}:{}:{}:{}",
                path.display(),
                line_no,
                model,
                u64_at(usage, &["total_tokens", "totalTokens", "total"]).unwrap_or(0)
            )
        });
        if !seen.insert(key) {
            summary.records_skipped += 1;
            diagnostics.push(Diagnostic::warning(
                Client::Gjc,
                "GjcDuplicateMessageSkipped",
                "duplicate assistant usage skipped",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
            continue;
        }
        let tokens = token_stats(usage);
        let component_total = tokens.input_total + tokens.output_total;
        if tokens.total_tokens != 0
            && component_total != 0
            && tokens.total_tokens != component_total
        {
            diagnostics.push(Diagnostic::warning(
                Client::Gjc,
                "GjcTotalMismatch",
                "totalTokens differs from input+output components",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
        }
        let branch = string_at(message, &["branch", "branch_id", "branchId"])
            .or_else(|| string_at(&v, &["branch", "branch_id", "branchId"]));
        let timestamp_raw = string_at(message, &["timestamp", "created_at", "createdAt"])
            .or_else(|| string_at(&v, &["timestamp", "created_at", "createdAt"]));
        let timestamp = match timestamp_raw {
            Some(raw) => match DateTime::parse_from_rfc3339(&raw) {
                Ok(dt) => Some(dt.with_timezone(&Utc)),
                Err(e) => {
                    diagnostics.push(Diagnostic::warning(
                        Client::Gjc,
                        "GjcInvalidTimestamp",
                        e.to_string(),
                        Some(path.to_path_buf()),
                        Some(line_no),
                    ));
                    None
                }
            },
            None => None,
        };
        records.push(UsageRecord {
            session_key: SessionKey {
                client: Client::Gjc,
                id: SessionId(session_id.clone()),
            },
            parent_session_key: parent_id.clone().map(|id| SessionKey {
                client: Client::Gjc,
                id: SessionId(id),
            }),
            message_id: msg_id.map(MessageId),
            source_path: path.to_path_buf(),
            source_line: Some(line_no),
            started_at: timestamp,
            session_name: title.clone(),
            model,
            reasoning_effort: resolve_effort(branch.as_deref(), &branch_effort, &branch_parent),
            tokens,
        });
        summary.records_emitted += 1;
    }
    Ok(())
}

fn resolve_effort(
    branch: Option<&str>,
    efforts: &HashMap<String, Option<ReasoningEffort>>,
    parents: &HashMap<String, String>,
) -> Option<ReasoningEffort> {
    let mut current = branch.unwrap_or("main").to_string();
    let mut guard = 0;
    loop {
        if let Some(effort) = efforts.get(&current) {
            return effort.clone();
        }
        let parent = parents.get(&current)?;
        current = parent.clone();
        guard += 1;
        if guard > 32 {
            return None;
        }
    }
}
