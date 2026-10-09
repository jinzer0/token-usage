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
    let mut session_id = path
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
        let payload = v.get("payload");
        if v.get("type").and_then(Value::as_str) == Some("session_meta")
            && let Some(id) = payload.and_then(|p| string_at(p, &["id"]))
        {
            session_id = id;
        }
        let context = if v.get("type").and_then(Value::as_str) == Some("turn_context") {
            payload
        } else {
            v.get("turn_context").or_else(|| v.get("turnContext"))
        };
        if let Some(ctx) = context {
            if let Some(next) = string_at(ctx, &["model"]) {
                model = Some(next);
            }
            if let Some(next) = string_at(ctx, &["reasoning_effort", "reasoningEffort", "effort"]) {
                effort = ReasoningEffort::parse(&next);
            }
        }
        let native_event = v.get("type").and_then(Value::as_str) == Some("event_msg")
            && payload.and_then(|p| p.get("type")).and_then(Value::as_str) == Some("token_count");
        let Some(usage) = (if native_event {
            payload
                .and_then(|p| p.get("info"))
                .filter(|info| !info.is_null())
        } else {
            v.get("token_count")
                .or_else(|| v.get("tokenCount"))
                .or_else(|| v.get("usage"))
        }) else {
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
        let cumulative = u64_at(
            usage.get("total_token_usage").unwrap_or(usage),
            &["total_tokens", "totalTokens", "total"],
        );
        if native_event
            && let (Some(total), Some(previous)) = (cumulative, previous_total)
            && total <= previous
        {
            summary.records_skipped += 1;
            diagnostics.push(Diagnostic::warning(
                Client::Codex,
                "CodexNonPositiveDelta",
                "repeated or reset cumulative token event skipped",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
            previous_total = Some(total);
            continue;
        }
        let tokens = if let Some(last) = usage
            .get("last_token_usage")
            .filter(|last| !last.is_null())
            .or_else(|| usage.get("lastTokenUsage").filter(|last| !last.is_null()))
        {
            token_stats(last)
        } else if let Some(total) = cumulative {
            match previous_total {
                Some(prev) if total > prev => {
                    let delta = total - prev;
                    Ok(TokenStats {
                        total_tokens: delta,
                        reasoning_has_unknown: true,
                        ..Default::default()
                    })
                }
                None => Ok(TokenStats {
                    total_tokens: total,
                    reasoning_has_unknown: true,
                    ..Default::default()
                }),
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
        let mut tokens = match tokens {
            Ok(tokens) => tokens,
            Err(error) => {
                summary.records_skipped += 1;
                diagnostics.push(Diagnostic::error(
                    Client::Codex,
                    "CodexTokenOverflow",
                    error.to_string(),
                    Some(path.to_path_buf()),
                    Some(line_no),
                ));
                continue;
            }
        };
        if tokens.total_tokens == 0 {
            tokens.total_tokens = tokens
                .input_total
                .checked_add(tokens.output_total)
                .context("token normalization overflow in input_total + output_total")?;
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

pub(crate) fn token_stats(v: &Value) -> Result<TokenStats> {
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
    let cache_write = u64_at(
        v,
        &["cache_write", "cacheWrite", "cache_write_input_tokens"],
    )
    .unwrap_or(0);
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
    let reasoning = u64_at(
        v,
        &[
            "reasoning_tokens",
            "reasoningTokens",
            "reasoning_output_tokens",
        ],
    );
    let component_total = input_total
        .checked_add(output_total)
        .context("token normalization overflow in input_total + output_total")?;
    let total_tokens =
        u64_at(v, &["total_tokens", "totalTokens", "total"]).unwrap_or(component_total);
    Ok(TokenStats {
        input_uncached: input_total.saturating_sub(cache_read),
        input_total,
        cache_read,
        cache_write,
        output_total,
        reasoning_known: reasoning.unwrap_or(0),
        reasoning_has_unknown: reasoning.is_none(),
        total_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cumulative_total_only_is_unknown_reasoning_not_known_zero() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let rows = [
            json!({"turn_context":{"model":"m"}}),
            json!({"token_count":{"total_tokens":10}}),
            json!({"token_count":{"total_tokens":15}}),
        ];
        std::fs::write(
            sessions.join("rollout-total-only.jsonl"),
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let result = parse(root.path()).unwrap();
        assert_eq!(
            result
                .records
                .iter()
                .map(|record| record.tokens.total_tokens)
                .collect::<Vec<_>>(),
            vec![10, 5]
        );
        assert!(
            result
                .records
                .iter()
                .all(|record| record.tokens.reasoning_has_unknown)
        );
        assert!(
            result
                .records
                .iter()
                .all(|record| record.tokens.reasoning_known == 0)
        );
    }

    #[test]
    fn normalization_rejects_component_overflow_even_with_explicit_total() {
        for total in [None, Some(1)] {
            let mut value = json!({"input_tokens": u64::MAX, "output_tokens": 1});
            if let Some(total) = total {
                value["total_tokens"] = json!(total);
            }
            assert!(
                token_stats(&value)
                    .unwrap_err()
                    .to_string()
                    .contains("overflow")
            );
        }
    }

    #[test]
    fn normalization_preserves_max_explicit_total_and_unknown_reasoning() {
        let tokens = token_stats(&json!({"total_tokens": u64::MAX})).unwrap();
        assert_eq!(tokens.total_tokens, u64::MAX);
        assert_eq!(tokens.input_total, 0);
        assert_eq!(tokens.output_total, 0);
        assert!(tokens.reasoning_has_unknown);
        let tokens = token_stats(&json!({
            "input_tokens": u64::MAX, "output_tokens": 0, "reasoning_tokens": 0
        }))
        .unwrap();
        assert_eq!(tokens.total_tokens, u64::MAX);
        assert!(!tokens.reasoning_has_unknown);
    }

    #[test]
    fn normalization_keeps_explicit_total_distinct_from_components() {
        let tokens = token_stats(&json!({
            "input_tokens": 5, "output_tokens": 3, "total_tokens": 11,
            "cache_read": 2, "cache_write": 1, "reasoning_tokens": 2
        }))
        .unwrap();
        assert_eq!(tokens.total_tokens, 11);
        assert_eq!(tokens.input_uncached, 3);
        assert_eq!(tokens.output_total, 3);
        assert_eq!(tokens.reasoning_known, 2);
    }

    #[test]
    fn overflowing_codex_record_has_source_diagnostic_and_next_record_survives() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let path = sessions.join("rollout-overflow.jsonl");
        let bad = json!({
            "model": "test", "usage": {"input_tokens": u64::MAX, "output_tokens": 1}
        });
        let good = json!({"model": "test", "usage": {"input_tokens": 3, "output_tokens": 2}});
        std::fs::write(&path, format!("{bad}\n{good}\n")).unwrap();
        let result = parse(root.path()).unwrap();
        assert_eq!(result.summary.records_skipped, 1);
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].tokens.total_tokens, 5);
        let diagnostic = &result.diagnostics[0];
        assert_eq!(diagnostic.code, "CodexTokenOverflow");
        assert!(matches!(
            diagnostic.severity,
            crate::diagnostics::Severity::Error
        ));
        assert_eq!(diagnostic.path.as_deref(), Some(path.as_path()));
        assert_eq!(diagnostic.line, Some(1));
    }

    #[test]
    fn overflowing_gjc_record_has_source_diagnostic_and_next_record_survives() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("overflow.jsonl");
        let bad = json!({
            "role": "assistant", "id": "bad", "model": "test",
            "usage": {"input_tokens": u64::MAX, "output_tokens": 1, "total_tokens": 1}
        });
        let good = json!({
            "role": "assistant", "id": "good", "model": "test",
            "usage": {"input_tokens": 3, "output_tokens": 2}
        });
        std::fs::write(&path, format!("{bad}\n{good}\n")).unwrap();
        let result = crate::sources::gjc::parse(root.path()).unwrap();
        assert_eq!(result.summary.records_skipped, 1);
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].tokens.total_tokens, 5);
        let diagnostic = &result.diagnostics[0];
        assert_eq!(diagnostic.code, "GjcTokenOverflow");
        assert!(matches!(
            diagnostic.severity,
            crate::diagnostics::Severity::Error
        ));
        assert_eq!(diagnostic.path.as_deref(), Some(path.as_path()));
        assert_eq!(diagnostic.line, Some(1));
    }

    #[test]
    fn native_null_last_usage_preserves_initial_and_later_cumulative_deltas() {
        for field in ["last_token_usage", "lastTokenUsage"] {
            let root = tempfile::tempdir().unwrap();
            let sessions = root.path().join("sessions");
            std::fs::create_dir(&sessions).unwrap();
            let event = |total, last: Value| {
                let mut info = json!({"total_token_usage": {"total_tokens": total}});
                info[field] = last;
                json!({"type":"event_msg","payload":{"type":"token_count","info":info}})
            };
            let values = [
                json!({"type":"session_meta","payload":{"id":"null-last"}}),
                json!({"type":"turn_context","payload":{"model":"m","effort":"high"}}),
                event(10, Value::Null),
                event(10, Value::Null),
                event(15, Value::Null),
                event(
                    20,
                    json!({"input_tokens":3,"output_tokens":2,"total_tokens":5,"reasoning_output_tokens":1}),
                ),
                event(25, Value::Null),
            ];
            std::fs::write(
                sessions.join("rollout-null.jsonl"),
                values
                    .iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            let result = parse(root.path()).unwrap();
            assert_eq!(
                result
                    .records
                    .iter()
                    .map(|r| r.tokens.total_tokens)
                    .collect::<Vec<_>>(),
                vec![10, 5, 5, 5],
                "{field}"
            );
            assert_eq!(result.summary.records_skipped, 1);
            for index in [0, 1, 3] {
                assert!(result.records[index].tokens.reasoning_has_unknown);
                assert_eq!(result.records[index].tokens.input_total, 0);
            }
            assert_eq!(result.records[2].tokens.input_total, 3);
            assert_eq!(result.records[2].tokens.output_total, 2);
            assert_eq!(result.records[2].tokens.reasoning_known, 1);
            assert!(!result.records[2].tokens.reasoning_has_unknown);
            let snapshot = crate::aggregate::aggregate(vec![result]).unwrap();
            assert_eq!(snapshot.totals.total_tokens, 25);
            assert_eq!(snapshot.sessions[0].models[0].tokens.total_tokens, 25);
            assert!(snapshot.totals.reasoning_has_unknown);
        }
    }

    #[test]
    fn null_snake_case_last_usage_does_not_mask_populated_camel_case_alias() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let values = [
            json!({"type":"turn_context","payload":{"model":"m"}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":{
                "total_token_usage":{"total_tokens":10},"last_token_usage":null,
                "lastTokenUsage":{"input_tokens":7,"output_tokens":3,"total_tokens":10,"reasoning_output_tokens":0}
            }}}),
        ];
        std::fs::write(
            sessions.join("rollout-alias.jsonl"),
            values
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let result = parse(root.path()).unwrap();
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].tokens.input_total, 7);
        assert_eq!(result.records[0].tokens.output_total, 3);
        assert_eq!(result.records[0].tokens.total_tokens, 10);
        assert!(!result.records[0].tokens.reasoning_has_unknown);
    }

    #[test]
    fn native_codex_payloads_keep_session_model_effort_and_skip_repeated_totals() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let event = |total, input, output| {
            json!({
                "type": "event_msg", "timestamp": "2026-10-08T00:00:00Z",
                "payload": {"type": "token_count", "info": {
                    "total_token_usage": {"total_tokens": total},
                    "last_token_usage": {
                        "input_tokens": input, "output_tokens": output,
                        "cached_input_tokens": 2, "cache_write_input_tokens": 1,
                        "reasoning_output_tokens": 1, "total_tokens": input + output
                    }
                }}
            })
        };
        let values = [
            json!({"type":"session_meta","payload":{"id":"native-id"}}),
            json!({"type":"turn_context","payload":{"model":"native-model","effort":"high"}}),
            event(10, 7, 3),
            event(10, 7, 3),
            event(15, 3, 2),
            json!({"type":"event_msg","payload":{"type":"token_count","info":null}}),
        ];
        let text = values
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(sessions.join("rollout-file-name.jsonl"), text).unwrap();
        let result = parse(root.path()).unwrap();
        assert_eq!(result.records.len(), 2);
        assert_eq!(result.summary.records_skipped, 1);
        assert_eq!(result.records[0].session_key.id.0, "native-id");
        assert_eq!(result.records[0].model, "native-model");
        assert_eq!(
            result.records[0].reasoning_effort,
            Some(ReasoningEffort::High)
        );
        assert_eq!(result.records[0].tokens.cache_read, 2);
        assert_eq!(result.records[0].tokens.cache_write, 1);
        assert_eq!(result.records[0].tokens.reasoning_known, 1);
        assert!(!result.records[0].tokens.reasoning_has_unknown);
        assert_eq!(
            result
                .records
                .iter()
                .map(|r| r.tokens.total_tokens)
                .sum::<u64>(),
            15
        );
    }
}
