use crate::{
    diagnostics::Diagnostic,
    domain::{Client, MessageId, ReasoningEffort, SessionId, SessionKey, TokenStats, UsageRecord},
    scan::{ParseResult, ScanSummary, SessionMetadata},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use walkdir::WalkDir;

const READ_BUF_CAPACITY: usize = 64 * 1024;

#[derive(Default)]
struct FileOutput {
    records: Vec<UsageRecord>,
    session_metadata: Vec<SessionMetadata>,
    diagnostics: Vec<Diagnostic>,
    files_scanned: u64,
    lines_read: u64,
    records_emitted: u64,
    records_skipped: u64,
}

pub fn parse(root: &Path) -> Result<ParseResult> {
    let mut summary = ScanSummary {
        client: Some(Client::Gjc),
        ..Default::default()
    };
    if !root.exists() {
        summary.missing_roots.push(root.to_path_buf());
        return Ok(ParseResult {
            records: vec![],
            session_metadata: vec![],
            diagnostics: vec![],
            summary,
        });
    }
    summary.roots_scanned.push(root.to_path_buf());

    let mut paths: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|v| v.to_str()) == Some("jsonl"))
        .map(|e| e.into_path())
        .collect();
    paths.sort();

    if paths.is_empty() {
        summary.empty_roots.push(root.to_path_buf());
        return Ok(ParseResult {
            records: vec![],
            session_metadata: vec![],
            diagnostics: vec![],
            summary,
        });
    }

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(paths.len());

    let slots: Vec<Mutex<Option<Result<FileOutput>>>> =
        (0..paths.len()).map(|_| Mutex::new(None)).collect();

    if workers <= 1 {
        for (slot, path) in slots.iter().zip(&paths) {
            *slot.lock().expect("slot lock not poisoned") = Some(parse_file(path));
        }
    } else {
        let next_idx = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                        if idx >= paths.len() {
                            break;
                        }
                        let res = parse_file(&paths[idx]);
                        *slots[idx].lock().expect("slot lock not poisoned") = Some(res);
                    }
                });
            }
        });
    }

    let mut records = Vec::new();
    let mut session_metadata = Vec::new();
    let mut diagnostics = Vec::new();
    for slot in slots {
        let output = slot
            .into_inner()
            .expect("slot lock not poisoned")
            .expect("every path slot populated")?;
        records.extend(output.records);
        session_metadata.extend(output.session_metadata);
        diagnostics.extend(output.diagnostics);
        summary.files_scanned += output.files_scanned;
        summary.lines_read += output.lines_read;
        summary.records_emitted += output.records_emitted;
        summary.records_skipped += output.records_skipped;
    }

    Ok(ParseResult {
        records,
        session_metadata,
        diagnostics,
        summary,
    })
}

fn parse_file(path: &Path) -> Result<FileOutput> {
    let mut output = FileOutput {
        files_scanned: 1,
        ..Default::default()
    };
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(READ_BUF_CAPACITY, file);
    let fallback_id = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut session_id = fallback_id;
    let mut parent_id = None::<String>;
    let mut title = None::<String>;
    let mut workspace = None::<String>;
    let mut metadata = BTreeMap::<SessionKey, SessionMetadata>::new();
    let mut branch_effort: HashMap<String, Option<ReasoningEffort>> = HashMap::new();
    let mut branch_parent: HashMap<String, String> = HashMap::new();
    let mut seen = HashSet::new();

    let mut line = String::new();
    let mut line_no: u64 = 0;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        line_no += 1;
        output.lines_read += 1;

        let parsed = match serde_json::from_str::<OptGjcLine>(&line) {
            Ok(OptGjcLine(Some(v))) => v,
            Ok(OptGjcLine(None)) => continue,
            Err(e) => {
                output.records_skipped += 1;
                output.diagnostics.push(Diagnostic::warning(
                    Client::Gjc,
                    "GjcMalformedLine",
                    e.to_string(),
                    Some(path.to_path_buf()),
                    Some(line_no),
                ));
                continue;
            }
        };

        let typ = parsed.typ();
        if matches!(typ, Some("header") | Some("session") | Some("header_patch")) {
            if let Some(id) = parsed.header_session_id() {
                session_id = id.to_string();
            }
            let next_title = parsed.header_title();
            if let Some(t) = next_title {
                title = Some(t.to_string());
            }
            let next_workspace = parsed.header_workspace();
            if let Some(w) = &next_workspace {
                workspace = Some(w.clone());
            }
            let key = SessionKey {
                client: Client::Gjc,
                id: SessionId(session_id.clone()),
            };
            let entry = metadata
                .entry(key.clone())
                .or_insert_with(|| SessionMetadata {
                    key,
                    parent_candidates: BTreeMap::new(),
                    name: None,
                    workspace: None,
                    source_path: path.to_path_buf(),
                    source_line: Some(line_no),
                });
            if let Some(t) = next_title {
                entry.name = Some(t.to_string());
            }
            if let Some(w) = next_workspace {
                entry.workspace = Some(w);
            }
            let mut next_parent = None;
            for p in parsed.parent_candidates() {
                next_parent.get_or_insert_with(|| p.to_string());
                entry.add_parent(
                    SessionKey {
                        client: Client::Gjc,
                        id: SessionId(p.to_string()),
                    },
                    path.to_path_buf(),
                    Some(line_no),
                );
            }
            if next_parent.is_some() {
                parent_id = next_parent;
            }
        }
        if matches!(typ, Some("thinking_level_change")) {
            let branch = parsed.branch().unwrap_or("main").to_string();
            if let Some(parent) = parsed.parent_branch() {
                branch_parent.insert(branch.clone(), parent.to_string());
            }
            if let Some(e) = parsed.effort()
                && e != "inherit"
            {
                branch_effort.insert(branch, ReasoningEffort::parse(e));
            }
            continue;
        }
        let msg = parsed.message.as_ref();
        let role = msg
            .and_then(|m| m.role.as_deref())
            .or(parsed.role.as_deref());
        if role != Some("assistant") && typ != Some("assistant") {
            continue;
        }
        let Some(usage) = msg.and_then(|m| m.usage.as_ref()).or(parsed.usage.as_ref()) else {
            continue;
        };
        let Some(model) = msg
            .and_then(|m| m.model.as_deref())
            .or(parsed.model.as_deref())
            .map(str::to_string)
        else {
            output.records_skipped += 1;
            output.diagnostics.push(Diagnostic::warning(
                Client::Gjc,
                "GjcMissingModel",
                "assistant usage skipped because model is missing",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
            continue;
        };
        let msg_id = msg
            .and_then(GjcMessage::msg_id)
            .or_else(|| parsed.top_msg_id())
            .map(str::to_string);
        let key = msg_id.clone().unwrap_or_else(|| {
            format!(
                "{}:{}:{}:{}",
                path.display(),
                line_no,
                model,
                usage.explicit_total().unwrap_or(0)
            )
        });
        if !seen.insert(key) {
            output.records_skipped += 1;
            output.diagnostics.push(Diagnostic::warning(
                Client::Gjc,
                "GjcDuplicateMessageSkipped",
                "duplicate assistant usage skipped",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
            continue;
        }
        let tokens = match normalize_usage(usage) {
            Ok(tokens) => tokens,
            Err(error) => {
                output.records_skipped += 1;
                output.diagnostics.push(Diagnostic::error(
                    Client::Gjc,
                    "GjcTokenOverflow",
                    error.to_string(),
                    Some(path.to_path_buf()),
                    Some(line_no),
                ));
                continue;
            }
        };
        // normalize_usage validates this sum before returning a record.
        let component_total = tokens.input_total + tokens.output_total;
        if tokens.total_tokens != 0
            && component_total != 0
            && tokens.total_tokens != component_total
        {
            output.diagnostics.push(Diagnostic::warning(
                Client::Gjc,
                "GjcTotalMismatch",
                "totalTokens differs from input+output components",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
        }
        let branch = msg.and_then(GjcMessage::branch).or_else(|| parsed.branch());
        let timestamp_raw = msg
            .and_then(GjcMessage::timestamp)
            .or_else(|| parsed.timestamp());
        let timestamp = match timestamp_raw {
            Some(raw) => match DateTime::parse_from_rfc3339(raw) {
                Ok(dt) => Some(dt.with_timezone(&Utc)),
                Err(e) => {
                    output.diagnostics.push(Diagnostic::warning(
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
        output.records.push(UsageRecord {
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
            workspace: workspace.clone(),
            model,
            reasoning_effort: resolve_effort(branch, &branch_effort, &branch_parent),
            tokens,
        });
        output.records_emitted += 1;
    }
    output.session_metadata.extend(metadata.into_values());
    Ok(output)
}

#[derive(Deserialize, Default)]
struct GjcLine {
    #[serde(rename = "type", default, deserialize_with = "opt_string")]
    typ: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    event: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    session_id: Option<String>,
    #[serde(rename = "sessionId", default, deserialize_with = "opt_string")]
    session_id_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    id: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    title: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    name: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    cwd: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    workspace: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    working_dir: Option<String>,
    #[serde(rename = "workingDir", default, deserialize_with = "opt_string")]
    working_dir_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    parent_session_id: Option<String>,
    #[serde(rename = "parentSessionId", default, deserialize_with = "opt_string")]
    parent_session_id_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    parent_id: Option<String>,
    #[serde(rename = "parentId", default, deserialize_with = "opt_string")]
    parent_id_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    branch: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    branch_id: Option<String>,
    #[serde(rename = "branchId", default, deserialize_with = "opt_string")]
    branch_id_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    parent_branch: Option<String>,
    #[serde(rename = "parentBranch", default, deserialize_with = "opt_string")]
    parent_branch_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    parent_branch_id: Option<String>,
    #[serde(rename = "parentBranchId", default, deserialize_with = "opt_string")]
    parent_branch_id_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    effort: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    thinking_level: Option<String>,
    #[serde(rename = "thinkingLevel", default, deserialize_with = "opt_string")]
    thinking_level_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    level: Option<String>,
    #[serde(default, deserialize_with = "opt_struct")]
    message: Option<GjcMessage>,
    #[serde(default, deserialize_with = "opt_string")]
    role: Option<String>,
    #[serde(default, deserialize_with = "opt_struct")]
    usage: Option<UsageObject>,
    #[serde(default, deserialize_with = "opt_string")]
    model: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    message_id: Option<String>,
    #[serde(rename = "messageId", default, deserialize_with = "opt_string")]
    message_id_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    timestamp: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    created_at: Option<String>,
    #[serde(rename = "createdAt", default, deserialize_with = "opt_string")]
    created_at_camel: Option<String>,
}

#[derive(Deserialize)]
#[serde(transparent)]
struct OptGjcLine(#[serde(deserialize_with = "opt_struct")] Option<GjcLine>);

impl GjcLine {
    fn typ(&self) -> Option<&str> {
        self.typ.as_deref().or(self.event.as_deref())
    }

    fn header_session_id(&self) -> Option<&str> {
        first_str(&[&self.session_id, &self.session_id_camel, &self.id])
    }

    fn header_title(&self) -> Option<&str> {
        first_str(&[&self.title, &self.name])
    }

    fn header_workspace(&self) -> Option<String> {
        [
            &self.cwd,
            &self.workspace,
            &self.working_dir,
            &self.working_dir_camel,
        ]
        .into_iter()
        .find_map(|opt| {
            opt.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
    }

    fn parent_candidates(&self) -> impl Iterator<Item = &str> {
        [
            &self.parent_session_id,
            &self.parent_session_id_camel,
            &self.parent_id,
            &self.parent_id_camel,
        ]
        .into_iter()
        .filter_map(|opt| opt.as_deref().filter(|p| !p.trim().is_empty()))
    }

    fn branch(&self) -> Option<&str> {
        first_str(&[&self.branch, &self.branch_id, &self.branch_id_camel])
    }

    fn parent_branch(&self) -> Option<&str> {
        first_str(&[
            &self.parent_branch,
            &self.parent_branch_camel,
            &self.parent_branch_id,
            &self.parent_branch_id_camel,
        ])
    }

    fn effort(&self) -> Option<&str> {
        first_str(&[
            &self.effort,
            &self.thinking_level,
            &self.thinking_level_camel,
            &self.level,
        ])
    }

    fn top_msg_id(&self) -> Option<&str> {
        first_str(&[&self.message_id, &self.message_id_camel, &self.id])
    }

    fn timestamp(&self) -> Option<&str> {
        first_str(&[&self.timestamp, &self.created_at, &self.created_at_camel])
    }
}

#[derive(Deserialize, Default)]
struct GjcMessage {
    #[serde(default, deserialize_with = "opt_string")]
    role: Option<String>,
    #[serde(default, deserialize_with = "opt_struct")]
    usage: Option<UsageObject>,
    #[serde(default, deserialize_with = "opt_string")]
    model: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    id: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    message_id: Option<String>,
    #[serde(rename = "messageId", default, deserialize_with = "opt_string")]
    message_id_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    branch: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    branch_id: Option<String>,
    #[serde(rename = "branchId", default, deserialize_with = "opt_string")]
    branch_id_camel: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    timestamp: Option<String>,
    #[serde(default, deserialize_with = "opt_string")]
    created_at: Option<String>,
    #[serde(rename = "createdAt", default, deserialize_with = "opt_string")]
    created_at_camel: Option<String>,
}

impl GjcMessage {
    fn msg_id(&self) -> Option<&str> {
        first_str(&[&self.id, &self.message_id, &self.message_id_camel])
    }

    fn branch(&self) -> Option<&str> {
        first_str(&[&self.branch, &self.branch_id, &self.branch_id_camel])
    }

    fn timestamp(&self) -> Option<&str> {
        first_str(&[&self.timestamp, &self.created_at, &self.created_at_camel])
    }
}

#[derive(Deserialize, Default)]
struct UsageObject {
    #[serde(default, deserialize_with = "opt_u64")]
    input_tokens: Option<u64>,
    #[serde(rename = "inputTokens", default, deserialize_with = "opt_u64")]
    input_tokens_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    input: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    prompt_tokens: Option<u64>,
    #[serde(rename = "promptTokens", default, deserialize_with = "opt_u64")]
    prompt_tokens_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    cache_read: Option<u64>,
    #[serde(rename = "cacheRead", default, deserialize_with = "opt_u64")]
    cache_read_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    cached_input_tokens: Option<u64>,
    #[serde(rename = "cachedInputTokens", default, deserialize_with = "opt_u64")]
    cached_input_tokens_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    cache_write: Option<u64>,
    #[serde(rename = "cacheWrite", default, deserialize_with = "opt_u64")]
    cache_write_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    cache_write_input_tokens: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    output_tokens: Option<u64>,
    #[serde(rename = "outputTokens", default, deserialize_with = "opt_u64")]
    output_tokens_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    output: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    completion_tokens: Option<u64>,
    #[serde(rename = "completionTokens", default, deserialize_with = "opt_u64")]
    completion_tokens_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    reasoning_tokens: Option<u64>,
    #[serde(rename = "reasoningTokens", default, deserialize_with = "opt_u64")]
    reasoning_tokens_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    reasoning_output_tokens: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    total_tokens: Option<u64>,
    #[serde(rename = "totalTokens", default, deserialize_with = "opt_u64")]
    total_tokens_camel: Option<u64>,
    #[serde(default, deserialize_with = "opt_u64")]
    total: Option<u64>,
}

impl UsageObject {
    fn explicit_total(&self) -> Option<u64> {
        self.total_tokens.or(self.total_tokens_camel).or(self.total)
    }

    fn to_token_stats(&self) -> Result<TokenStats> {
        let input_total = self
            .input_tokens
            .or(self.input_tokens_camel)
            .or(self.input)
            .or(self.prompt_tokens)
            .or(self.prompt_tokens_camel)
            .unwrap_or(0);
        let cache_read = self
            .cache_read
            .or(self.cache_read_camel)
            .or(self.cached_input_tokens)
            .or(self.cached_input_tokens_camel)
            .unwrap_or(0);
        let cache_write = self
            .cache_write
            .or(self.cache_write_camel)
            .or(self.cache_write_input_tokens)
            .unwrap_or(0);
        let output_total = self
            .output_tokens
            .or(self.output_tokens_camel)
            .or(self.output)
            .or(self.completion_tokens)
            .or(self.completion_tokens_camel)
            .unwrap_or(0);
        let reasoning = self
            .reasoning_tokens
            .or(self.reasoning_tokens_camel)
            .or(self.reasoning_output_tokens);
        let component_total = input_total
            .checked_add(output_total)
            .context("token normalization overflow in input_total + output_total")?;
        let total_tokens = self.explicit_total().unwrap_or(component_total);
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
}

fn first_str<'a>(candidates: &[&'a Option<String>]) -> Option<&'a str> {
    candidates.iter().find_map(|opt| opt.as_deref())
}

/// Deserialize an optional string while tolerating null or unexpected JSON types without erroring.
fn opt_string<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptStringVisitor;

    impl<'de> serde::de::Visitor<'de> for OptStringVisitor {
        type Value = Option<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an optional string or ignorable JSON value")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
            Ok(Some(v.to_owned()))
        }

        fn visit_string<E: serde::de::Error>(
            self,
            v: String,
        ) -> std::result::Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2: serde::Deserializer<'de>>(
            self,
            deserializer: D2,
        ) -> std::result::Result<Self::Value, D2::Error> {
            deserializer.deserialize_any(self)
        }

        fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            Ok(None)
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while map
                .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                .is_some()
            {}
            Ok(None)
        }
    }

    deserializer.deserialize_any(OptStringVisitor)
}

/// Deserialize an optional u64 while tolerating null or non-u64 JSON types without erroring.
fn opt_u64<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptU64Visitor;

    impl<'de> serde::de::Visitor<'de> for OptU64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an optional u64 or ignorable JSON value")
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
            Ok(u64::try_from(v).ok())
        }

        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2: serde::Deserializer<'de>>(
            self,
            deserializer: D2,
        ) -> std::result::Result<Self::Value, D2::Error> {
            deserializer.deserialize_any(self)
        }

        fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            Ok(None)
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while map
                .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                .is_some()
            {}
            Ok(None)
        }
    }

    deserializer.deserialize_any(OptU64Visitor)
}

/// Deserialize an optional JSON object into `T`, ignoring null or non-object JSON types.
fn opt_struct<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct OptStructVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for OptStructVisitor<T> {
        type Value = Option<T>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an optional JSON object or ignorable JSON value")
        }

        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2: serde::Deserializer<'de>>(
            self,
            deserializer: D2,
        ) -> std::result::Result<Self::Value, D2::Error> {
            deserializer.deserialize_any(self)
        }

        fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            Ok(None)
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            T::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(Some)
        }
    }

    deserializer.deserialize_any(OptStructVisitor(std::marker::PhantomData))
}

fn normalize_usage(usage: &UsageObject) -> Result<TokenStats> {
    let mut tokens = usage.to_token_stats()?;
    // GJC's native `input` is uncached; cacheRead/cacheWrite are separate buckets.
    if let Some(input) = usage.input {
        tokens.input_uncached = input;
        tokens.input_total = input
            .checked_add(tokens.cache_read)
            .and_then(|total| total.checked_add(tokens.cache_write))
            .context("token normalization overflow in input + cache_read + cache_write")?;
        let component_total = tokens
            .input_total
            .checked_add(tokens.output_total)
            .context("token normalization overflow in input_total + output_total")?;
        if usage.explicit_total().is_none() {
            tokens.total_tokens = component_total;
        }
    }
    Ok(tokens)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn normalize_usage_value(value: Value) -> Result<TokenStats> {
        let usage: UsageObject = serde_json::from_value(value)?;
        normalize_usage(&usage)
    }

    #[test]
    fn metadata_without_usage_keeps_all_parents_and_latest_title_patch() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("child.jsonl"), [
            json!({"type":"header","session_id":"child","parent_session_id":"a","parentId":"b","title":"Old"}),
            json!({"type":"header_patch","parent_session_id":null,"title":"Latest"}),
            json!({"type":"header_patch","parent_session_id":"a","title":null}),
        ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        let result = parse(root.path()).unwrap();
        assert!(result.records.is_empty());
        assert_eq!(result.summary.records_emitted, 0);
        let metadata = &result.session_metadata[0];
        assert_eq!(metadata.key.id.0, "child");
        assert_eq!(metadata.name.as_deref(), Some("Latest"));
        assert_eq!(metadata.parent_candidates.len(), 2);
        assert!(
            metadata
                .parent_candidates
                .values()
                .all(|(_, line)| *line == Some(1))
        );
    }

    #[test]
    fn late_title_patch_updates_metadata_without_changing_raw_usage_parent() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("child.jsonl"), [
            json!({"type":"header","session_id":"child","parent_session_id":"root","title":"Before"}),
            json!({"type":"assistant","message":{"id":"message","model":"m","usage":{"input":3,"output":2}}}),
            json!({"type":"header_patch","title":"After"}),
        ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        let result = parse(root.path()).unwrap();
        assert_eq!(
            result.records[0].parent_session_key.as_ref().unwrap().id.0,
            "root"
        );
        assert_eq!(result.records[0].session_name.as_deref(), Some("Before"));
        assert_eq!(result.session_metadata[0].name.as_deref(), Some("After"));
        assert_eq!(result.records[0].tokens.total_tokens, 5);
    }

    #[test]
    fn native_input_and_cache_are_disjoint_and_output_does_not_double_count_reasoning() {
        let tokens = normalize_usage_value(json!({
            "input": 535, "output": 149, "cacheRead": 22784,
            "cacheWrite": 3, "totalTokens": 23471, "reasoningTokens": 20
        }))
        .unwrap();
        assert_eq!(tokens.input_uncached, 535);
        assert_eq!(tokens.input_total, 23322);
        assert_eq!(tokens.output_total, 149);
        assert_eq!(tokens.total_tokens, 23471);
        assert_eq!(tokens.reasoning_known, 20);
    }

    #[test]
    fn native_cache_overflow_is_rejected_and_explicit_total_is_preserved() {
        assert!(
            normalize_usage_value(json!({
                "input": u64::MAX, "cacheRead": 1, "totalTokens": 1
            }))
            .is_err()
        );
        let tokens = normalize_usage_value(json!({
            "input": 3, "cacheRead": 5, "cacheWrite": 2,
            "output": 4, "totalTokens": 17
        }))
        .unwrap();
        assert_eq!(tokens.input_total, 10);
        assert_eq!(tokens.total_tokens, 17);
        assert!(tokens.reasoning_has_unknown);
        let tokens = normalize_usage_value(json!({
            "input": 3, "cacheRead": 5, "cacheWrite": 2, "output": 4
        }))
        .unwrap();
        assert_eq!(tokens.total_tokens, 14);
    }

    #[test]
    fn header_cwd_and_patches_propagate_to_records_and_metadata() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("first.jsonl"),
            [
                json!({"type":"header","session_id":"s1","cwd":"  /work/gjc-initial  "}),
                json!({"type":"assistant","message":{"id":"m1","model":"m","usage":{"input":3,"output":2}}}),
                json!({"type":"header_patch","cwd":"/work/gjc-updated"}),
                json!({"type":"header_patch","cwd":"   "}),
                json!({"type":"header_patch","cwd":null}),
                json!({"type":"assistant","message":{"id":"m2","model":"m","usage":{"input":1,"output":1}}}),
            ]
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        )
        .unwrap();
        std::fs::write(
            root.path().join("second.jsonl"),
            [
                json!({"type":"session","id":"s2","cwd":"","workingDir":" /work/gjc-fallback "}),
                json!({"type":"assistant","message":{"id":"m3","model":"m","usage":{"input":2,"output":2}}}),
            ]
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        )
        .unwrap();
        let result = parse(root.path()).unwrap();
        assert_eq!(result.records.len(), 3);
        let m1 = result
            .records
            .iter()
            .find(|r| r.message_id.as_ref().unwrap().0 == "m1")
            .unwrap();
        let m2 = result
            .records
            .iter()
            .find(|r| r.message_id.as_ref().unwrap().0 == "m2")
            .unwrap();
        let m3 = result
            .records
            .iter()
            .find(|r| r.message_id.as_ref().unwrap().0 == "m3")
            .unwrap();
        assert_eq!(m1.workspace.as_deref(), Some("/work/gjc-initial"));
        assert_eq!(m2.workspace.as_deref(), Some("/work/gjc-updated"));
        assert_eq!(m3.workspace.as_deref(), Some("/work/gjc-fallback"));
        let s1_meta = result
            .session_metadata
            .iter()
            .find(|m| m.key.id.0 == "s1")
            .unwrap();
        assert_eq!(s1_meta.workspace.as_deref(), Some("/work/gjc-updated"));
        let s2_meta = result
            .session_metadata
            .iter()
            .find(|m| m.key.id.0 == "s2")
            .unwrap();
        assert_eq!(s2_meta.workspace.as_deref(), Some("/work/gjc-fallback"));
    }

    #[test]
    fn unexpected_optional_field_types_and_large_unknown_payloads_do_not_trigger_malformed() {
        let root = tempfile::tempdir().unwrap();
        let huge_text = "x".repeat(80 * 1024);
        std::fs::write(
            root.path().join("tolerant.jsonl"),
            [
                json!({"type":"header","session_id":"s-tol","parent_session_id":123,"parentId":["bad"],"title":{"nested":true},"cwd":false}),
                json!({"type":"user","message":{"role":"user","content":huge_text,"tool_results":[{"output":huge_text}]}}),
                json!({"type":"message","message":"string-not-object","role":"user"}),
                json!({"type":"assistant","id":"top-only","model":"m","usage":{"input":"bad","output":4,"cacheRead":null}}),
                json!({"type":"thinking_level_change","branch":42,"parent_branch":["ignored"],"effort":"high"}),
                json!({"type":"assistant","message":{"messageId":"msg-2","model":"m2","usage":{"input":2,"output":3}}}),
            ]
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        )
        .unwrap();

        let result = parse(root.path()).unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        assert_eq!(result.summary.lines_read, 6);
        assert_eq!(result.summary.records_emitted, 2);
        assert_eq!(result.summary.records_skipped, 0);
        assert_eq!(result.records[0].message_id.as_ref().unwrap().0, "top-only");
        assert_eq!(result.records[0].tokens.output_total, 4);
        assert_eq!(result.records[1].message_id.as_ref().unwrap().0, "msg-2");
        assert_eq!(
            result.records[1].reasoning_effort,
            Some(ReasoningEffort::High)
        );
    }

    #[test]
    fn parallel_multi_file_parsing_is_complete_and_deterministic() {
        let root = tempfile::tempdir().unwrap();
        let sub_a = root.path().join("a");
        let sub_b = root.path().join("b");
        std::fs::create_dir_all(&sub_a).unwrap();
        std::fs::create_dir_all(&sub_b).unwrap();

        let file_count = 12usize;
        for i in 0..file_count {
            let dir = if i % 2 == 0 { &sub_a } else { &sub_b };
            let path = dir.join(format!("session-{i:02}.jsonl"));
            let mut lines = vec![
                json!({"type":"header","session_id":format!("s-{i:02}"),"title":format!("Title {i:02}")}).to_string(),
                json!({"type":"user","message":{"role":"user","content":"ignored payload"}}).to_string(),
                json!({"type":"assistant","message":{"id":format!("m-{i:02}"),"model":"m","usage":{"input":i + 1,"output":2}}}).to_string(),
            ];
            if i % 3 == 0 {
                lines.push("not-valid-json".to_string());
            }
            std::fs::write(path, lines.join("\n")).unwrap();
        }
        std::fs::write(root.path().join("ignored.txt"), "not jsonl").unwrap();

        let first = parse(root.path()).unwrap();
        assert_eq!(first.summary.files_scanned, file_count as u64);
        assert_eq!(first.summary.records_emitted, file_count as u64);
        assert_eq!(first.summary.records_skipped, 4);
        assert_eq!(first.records.len(), file_count);
        assert_eq!(first.session_metadata.len(), file_count);
        assert_eq!(first.diagnostics.len(), 4);

        for window in first.records.windows(2) {
            assert!(window[0].source_path < window[1].source_path);
        }
        for window in first.session_metadata.windows(2) {
            assert!(window[0].source_path < window[1].source_path);
        }
        for window in first.diagnostics.windows(2) {
            assert!(window[0].path < window[1].path);
        }

        for _ in 0..5 {
            let rerun = parse(root.path()).unwrap();
            assert_eq!(
                rerun
                    .records
                    .iter()
                    .map(|r| (&r.source_path, &r.session_key.id.0, r.tokens.total_tokens))
                    .collect::<Vec<_>>(),
                first
                    .records
                    .iter()
                    .map(|r| (&r.source_path, &r.session_key.id.0, r.tokens.total_tokens))
                    .collect::<Vec<_>>(),
            );
            assert_eq!(
                rerun
                    .session_metadata
                    .iter()
                    .map(|m| (&m.source_path, &m.key.id.0))
                    .collect::<Vec<_>>(),
                first
                    .session_metadata
                    .iter()
                    .map(|m| (&m.source_path, &m.key.id.0))
                    .collect::<Vec<_>>(),
            );
            assert_eq!(
                rerun
                    .diagnostics
                    .iter()
                    .map(|d| (&d.path, &d.code, d.line))
                    .collect::<Vec<_>>(),
                first
                    .diagnostics
                    .iter()
                    .map(|d| (&d.path, &d.code, d.line))
                    .collect::<Vec<_>>(),
            );
            assert_eq!(rerun.summary.files_scanned, first.summary.files_scanned);
            assert_eq!(rerun.summary.lines_read, first.summary.lines_read);
            assert_eq!(rerun.summary.records_emitted, first.summary.records_emitted);
            assert_eq!(rerun.summary.records_skipped, first.summary.records_skipped);
        }
    }
}
