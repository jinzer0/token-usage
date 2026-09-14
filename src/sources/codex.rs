use crate::{
    diagnostics::Diagnostic,
    domain::{Client, MessageId, ReasoningEffort, SessionId, SessionKey, TokenStats, UsageRecord},
    scan::{ParseResult, ScanSummary},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};
use walkdir::WalkDir;

pub fn parse(root: &Path) -> Result<ParseResult> {
    let mut summary = ScanSummary {
        client: Some(Client::Codex),
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
    let names = read_index(root, &mut summary);
    let sessions = root.join("sessions");
    if !sessions.exists() {
        summary.empty_roots.push(root.to_path_buf());
        return Ok(ParseResult {
            records: vec![],
            diagnostics: vec![],
            summary,
        });
    }
    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    let mut files = 0;
    for entry in WalkDir::new(&sessions)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        if entry.path().extension().and_then(|v| v.to_str()) != Some("jsonl") {
            continue;
        }
        files += 1;
        parse_file(
            entry.path(),
            &names,
            &mut records,
            &mut diagnostics,
            &mut summary,
        )?;
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

fn read_index(root: &Path, summary: &mut ScanSummary) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let path = root.join("session_index.jsonl");
    let Ok(file) = File::open(&path) else {
        return names;
    };
    summary.files_scanned += 1;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        summary.lines_read += 1;
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            let id = string_at(&v, &["id", "session_id", "sessionId"]);
            let name = string_at(&v, &["name", "title"]);
            if let (Some(id), Some(name)) = (id, name) {
                names.insert(id, name);
            }
        }
    }
    names
}

fn parse_file(
    path: &Path,
    names: &HashMap<String, String>,
    records: &mut Vec<UsageRecord>,
    diagnostics: &mut Vec<Diagnostic>,
    summary: &mut ScanSummary,
) -> Result<()> {
    summary.files_scanned += 1;
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let session_id = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown")
        .trim_start_matches("rollout-")
        .to_string();
    let mut model = None;
    let mut effort = None;
    let mut previous_total = None::<u64>;
    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line_no = idx as u64 + 1;
        summary.lines_read += 1;
        let line = line?;
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                summary.records_skipped += 1;
                diagnostics.push(Diagnostic::warning(
                    Client::Codex,
                    "CodexMalformedLine",
                    e.to_string(),
                    Some(path.to_path_buf()),
                    Some(line_no),
                ));
                continue;
            }
        };
        if let Some(ctx) = v.get("turn_context").or_else(|| v.get("turnContext")) {
            if let Some(next) = string_at(ctx, &["model"]) {
                model = Some(next);
            }
            if let Some(next) = string_at(ctx, &["reasoning_effort", "reasoningEffort", "effort"]) {
                effort = ReasoningEffort::parse(&next);
            }
        }
        if v.get("info").is_some_and(Value::is_null) {
            continue;
        }
        let Some(usage) = v
            .get("token_count")
            .or_else(|| v.get("tokenCount"))
            .or_else(|| v.get("usage"))
        else {
            continue;
        };
        let Some(model) = model.clone().or_else(|| string_at(&v, &["model"])) else {
            summary.records_skipped += 1;
            diagnostics.push(Diagnostic::warning(
                Client::Codex,
                "CodexMissingModel",
                "token event skipped because model is unknown",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
            continue;
        };
        let cumulative = u64_at(usage, &["total_tokens", "totalTokens", "total"]);
        let tokens = if let Some(last) = usage
            .get("last_token_usage")
            .or_else(|| usage.get("lastTokenUsage"))
        {
            token_stats(last)
        } else if let Some(total) = cumulative {
            match previous_total {
                Some(prev) if total > prev => {
                    let delta = total - prev;
                    TokenStats {
                        total_tokens: delta,
                        ..Default::default()
                    }
                }
                None => TokenStats {
                    total_tokens: total,
                    ..Default::default()
                },
                _ => {
                    summary.records_skipped += 1;
                    diagnostics.push(Diagnostic::warning(
                        Client::Codex,
                        "CodexNonPositiveDelta",
                        "stale or reset cumulative token counter skipped",
                        Some(path.to_path_buf()),
                        Some(line_no),
                    ));
                    previous_total = Some(total);
                    continue;
                }
            }
        } else {
            token_stats(usage)
        };
        if let Some(total) = cumulative {
            previous_total = Some(total);
        }
        let mut tokens = tokens;
        if tokens.total_tokens == 0 {
            tokens.total_tokens = tokens.input_total + tokens.output_total;
        }
        if tokens.input_uncached == 0 {
            tokens.input_uncached = tokens.input_total.saturating_sub(tokens.cache_read);
        }
        let timestamp = match string_at(&v, &["timestamp", "created_at", "createdAt"]) {
            Some(raw) => match DateTime::parse_from_rfc3339(&raw) {
                Ok(dt) => Some(dt.with_timezone(&Utc)),
                Err(e) => {
                    diagnostics.push(Diagnostic::warning(
                        Client::Codex,
                        "CodexInvalidTimestamp",
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
                client: Client::Codex,
                id: SessionId(session_id.clone()),
            },
            parent_session_key: None,
            message_id: string_at(&v, &["id", "message_id", "messageId"]).map(MessageId),
            source_path: path.to_path_buf(),
            source_line: Some(line_no),
            started_at: timestamp,
            session_name: names.get(&session_id).cloned(),
            model,
            reasoning_effort: effort.clone(),
            tokens,
        });
        summary.records_emitted += 1;
    }
    Ok(())
}

pub(crate) fn string_at(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k)?.as_str().map(str::to_string))
}

pub(crate) fn u64_at(v: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| v.get(*k)?.as_u64())
}

pub(crate) fn token_stats(v: &Value) -> TokenStats {
    let input_total = u64_at(
        v,
        &[
            "input_tokens",
            "inputTokens",
            "input",
            "prompt_tokens",
            "promptTokens",
        ],
    )
    .unwrap_or(0);
    let cache_read = u64_at(
        v,
        &[
            "cache_read",
            "cacheRead",
            "cached_input_tokens",
            "cachedInputTokens",
        ],
    )
    .unwrap_or(0);
    let cache_write = u64_at(v, &["cache_write", "cacheWrite"]).unwrap_or(0);
    let output_total = u64_at(
        v,
        &[
            "output_tokens",
            "outputTokens",
            "output",
            "completion_tokens",
            "completionTokens",
        ],
    )
    .unwrap_or(0);
    let reasoning = u64_at(v, &["reasoning_tokens", "reasoningTokens"]);
    let total_tokens =
        u64_at(v, &["total_tokens", "totalTokens", "total"]).unwrap_or(input_total + output_total);
    TokenStats {
        input_uncached: input_total.saturating_sub(cache_read),
        input_total,
        cache_read,
        cache_write,
        output_total,
        reasoning_known: reasoning.unwrap_or(0),
        reasoning_has_unknown: reasoning.is_none(),
        total_tokens,
    }
}
