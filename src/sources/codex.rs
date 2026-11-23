use crate::{
    diagnostics::Diagnostic,
    domain::{Client, MessageId, ReasoningEffort, SessionId, SessionKey, TokenStats, UsageRecord},
    scan::{ParseResult, ScanSummary, SessionMetadata},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::de::{
    Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor, value::MapAccessDeserializer,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use walkdir::WalkDir;

const READ_BUFFER_CAPACITY: usize = 64 * 1024;

pub fn parse(root: &Path) -> Result<ParseResult> {
    let mut summary = ScanSummary {
        client: Some(Client::Codex),
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
    let names = read_index(root, &mut summary);
    let sessions = root.join("sessions");
    if !sessions.exists() {
        summary.empty_roots.push(root.to_path_buf());
        return Ok(ParseResult {
            records: vec![],
            session_metadata: vec![],
            diagnostics: vec![],
            summary,
        });
    }
    let mut paths: Vec<PathBuf> = WalkDir::new(&sessions)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|v| v.to_str()) == Some("jsonl"))
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

    let mut records = Vec::new();
    let mut session_metadata = Vec::new();
    let mut diagnostics = Vec::new();
    if workers <= 1 {
        for path in &paths {
            let output = parse_file(path, &names)?;
            merge_output(
                output,
                &mut records,
                &mut session_metadata,
                &mut diagnostics,
                &mut summary,
            );
        }
    } else {
        for output in parse_files_parallel(&paths, &names, workers)? {
            merge_output(
                output,
                &mut records,
                &mut session_metadata,
                &mut diagnostics,
                &mut summary,
            );
        }
    }
    Ok(ParseResult {
        records,
        session_metadata,
        diagnostics,
        summary,
    })
}

#[derive(Default)]
struct FileParseOutput {
    records: Vec<UsageRecord>,
    session_metadata: Vec<SessionMetadata>,
    diagnostics: Vec<Diagnostic>,
    files_scanned: u64,
    lines_read: u64,
    records_emitted: u64,
    records_skipped: u64,
}

fn merge_output(
    output: FileParseOutput,
    records: &mut Vec<UsageRecord>,
    session_metadata: &mut Vec<SessionMetadata>,
    diagnostics: &mut Vec<Diagnostic>,
    summary: &mut ScanSummary,
) {
    records.extend(output.records);
    session_metadata.extend(output.session_metadata);
    diagnostics.extend(output.diagnostics);
    summary.files_scanned += output.files_scanned;
    summary.lines_read += output.lines_read;
    summary.records_emitted += output.records_emitted;
    summary.records_skipped += output.records_skipped;
}

fn parse_files_parallel(
    paths: &[PathBuf],
    names: &HashMap<String, String>,
    workers: usize,
) -> Result<Vec<FileParseOutput>> {
    let next_idx = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let mut slots: Vec<Option<Result<FileParseOutput>>> = (0..paths.len()).map(|_| None).collect();

    let worker_results: Vec<Vec<(usize, Result<FileParseOutput>)>> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            handles.push(scope.spawn(|| {
                let mut local = Vec::new();
                while !cancelled.load(Ordering::Relaxed) {
                    let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                    if idx >= paths.len() {
                        break;
                    }
                    match parse_file(&paths[idx], names) {
                        Ok(out) => local.push((idx, Ok(out))),
                        Err(err) => {
                            cancelled.store(true, Ordering::Relaxed);
                            local.push((idx, Err(err)));
                            break;
                        }
                    }
                }
                local
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("codex parser worker panicked"))
            .collect()
    });

    for batch in worker_results {
        for (idx, result) in batch {
            slots[idx] = Some(result);
        }
    }

    let mut outputs = Vec::with_capacity(paths.len());
    for slot in slots.into_iter().flatten() {
        outputs.push(slot?);
    }
    Ok(outputs)
}

fn read_index(root: &Path, summary: &mut ScanSummary) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let path = root.join("session_index.jsonl");
    let Ok(file) = File::open(&path) else {
        return names;
    };
    summary.files_scanned += 1;
    let mut reader = BufReader::with_capacity(READ_BUFFER_CAPACITY, file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        summary.lines_read += 1;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Ok(MaybeMap(Some(entry))) = serde_json::from_str::<MaybeMap<IndexLine>>(trimmed) {
            let id = entry.id.or(entry.session_id).or(entry.session_id_camel);
            let name = entry.name.or(entry.title);
            if let (Some(id), Some(name)) = (id, name) {
                names.insert(id, name);
            }
        }
    }
    names
}

fn parse_file(path: &Path, names: &HashMap<String, String>) -> Result<FileParseOutput> {
    let mut output = FileParseOutput {
        files_scanned: 1,
        ..Default::default()
    };
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut session_id = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown")
        .trim_start_matches("rollout-")
        .to_string();
    let mut model = None;
    let mut effort = None;
    let mut workspace = None::<String>;
    let mut previous_total = None::<u64>;
    let mut metadata = BTreeMap::<SessionKey, SessionMetadata>::new();

    let mut reader = BufReader::with_capacity(READ_BUFFER_CAPACITY, file);
    let mut line = String::new();
    let mut line_no: u64 = 0;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        line_no += 1;
        output.lines_read += 1;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let parsed = match serde_json::from_str::<MaybeMap<CodexLine>>(trimmed) {
            Ok(MaybeMap(Some(v))) => v,
            Ok(MaybeMap(None)) => continue,
            Err(e) => {
                output.records_skipped += 1;
                output.diagnostics.push(Diagnostic::warning(
                    Client::Codex,
                    "CodexMalformedLine",
                    e.to_string(),
                    Some(path.to_path_buf()),
                    Some(line_no),
                ));
                continue;
            }
        };

        let payload = parsed.payload.0.as_ref();
        if parsed.r#type.as_deref() == Some("session_meta")
            && let Some(payload) = payload
        {
            let extracted_workspace = payload.workspace();
            if let Some(id) = payload.id.as_deref().filter(|id| !id.trim().is_empty()) {
                session_id = id.to_string();
                let key = SessionKey {
                    client: Client::Codex,
                    id: SessionId(session_id.clone()),
                };
                let entry = metadata
                    .entry(key.clone())
                    .or_insert_with(|| SessionMetadata {
                        key,
                        parent_candidates: BTreeMap::new(),
                        name: names.get(&session_id).cloned(),
                        workspace: None,
                        source_path: path.to_path_buf(),
                        source_line: Some(line_no),
                    });
                if entry.workspace.is_none() {
                    entry.workspace = extracted_workspace;
                }
                workspace = entry.workspace.clone();
                for parent in [
                    payload.parent_thread_id.as_deref(),
                    payload.nested_parent_thread_id(),
                ] {
                    if let Some(parent) = parent.filter(|p| !p.trim().is_empty()) {
                        entry.add_parent(
                            SessionKey {
                                client: Client::Codex,
                                id: SessionId(parent.to_string()),
                            },
                            path.to_path_buf(),
                            Some(line_no),
                        );
                    }
                }
            } else if workspace.is_none() {
                workspace = extracted_workspace;
            }
        }

        if parsed.r#type.as_deref() == Some("turn_context") {
            if let Some(payload) = payload {
                apply_turn_context(
                    payload.model.as_deref(),
                    payload.effort_str(),
                    &mut model,
                    &mut effort,
                );
            }
        } else if let Some(ctx) = parsed
            .turn_context
            .0
            .as_ref()
            .or(parsed.turn_context_camel.0.as_ref())
        {
            apply_turn_context(
                ctx.model.as_deref(),
                ctx.effort_str(),
                &mut model,
                &mut effort,
            );
        }

        let native_event = parsed.r#type.as_deref() == Some("event_msg")
            && payload.and_then(|p| p.r#type.as_deref()) == Some("token_count");
        let Some(usage) = (if native_event {
            payload.and_then(|p| p.info.0.as_ref())
        } else {
            parsed
                .token_count
                .0
                .as_ref()
                .or(parsed.token_count_camel.0.as_ref())
                .or(parsed.usage.0.as_ref())
        }) else {
            continue;
        };

        let Some(model) = model.clone().or(parsed.model) else {
            output.records_skipped += 1;
            output.diagnostics.push(Diagnostic::warning(
                Client::Codex,
                "CodexMissingModel",
                "token event skipped because model is unknown",
                Some(path.to_path_buf()),
                Some(line_no),
            ));
            continue;
        };

        let cumulative = match &usage.total_token_usage {
            Some(MaybeMap(Some(total_usage))) => total_usage.explicit_total(),
            Some(MaybeMap(None)) => None,
            None => usage.explicit_total(),
        };
        if native_event
            && let (Some(total), Some(previous)) = (cumulative, previous_total)
            && total <= previous
        {
            output.records_skipped += 1;
            output.diagnostics.push(Diagnostic::warning(
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
            .last_token_usage
            .0
            .as_ref()
            .or(usage.last_token_usage_camel.0.as_ref())
        {
            last.to_token_stats()
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
                    output.records_skipped += 1;
                    output.diagnostics.push(Diagnostic::warning(
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
            usage.to_token_stats()
        };
        if let Some(total) = cumulative {
            previous_total = Some(total);
        }
        let mut tokens = match tokens {
            Ok(tokens) => tokens,
            Err(error) => {
                output.records_skipped += 1;
                output.diagnostics.push(Diagnostic::error(
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
        let timestamp = match parsed
            .timestamp
            .or(parsed.created_at)
            .or(parsed.created_at_camel)
        {
            Some(raw) => match DateTime::parse_from_rfc3339(&raw) {
                Ok(dt) => Some(dt.with_timezone(&Utc)),
                Err(e) => {
                    output.diagnostics.push(Diagnostic::warning(
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
        let message_id = parsed
            .id
            .or(parsed.message_id)
            .or(parsed.message_id_camel)
            .map(MessageId);
        output.records.push(UsageRecord {
            session_key: SessionKey {
                client: Client::Codex,
                id: SessionId(session_id.clone()),
            },
            parent_session_key: None,
            message_id,
            source_path: path.to_path_buf(),
            source_line: Some(line_no),
            started_at: timestamp,
            session_name: names.get(&session_id).cloned(),
            workspace: workspace.clone(),
            model,
            reasoning_effort: effort.clone(),
            tokens,
        });
        output.records_emitted += 1;
    }
    output.session_metadata.extend(metadata.into_values());
    Ok(output)
}

fn apply_turn_context(
    model_val: Option<&str>,
    effort_val: Option<&str>,
    model: &mut Option<String>,
    effort: &mut Option<ReasoningEffort>,
) {
    if let Some(next) = model_val {
        *model = Some(next.to_string());
    }
    if let Some(next) = effort_val {
        *effort = ReasoningEffort::parse(next);
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct IndexLine {
    #[serde(deserialize_with = "deserialize_lossy_string")]
    id: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    session_id: Option<String>,
    #[serde(rename = "sessionId", deserialize_with = "deserialize_lossy_string")]
    session_id_camel: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    name: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    title: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexLine {
    #[serde(deserialize_with = "deserialize_lossy_string")]
    r#type: Option<String>,
    payload: MaybeMap<CodexPayload>,
    turn_context: MaybeMap<TurnContext>,
    #[serde(rename = "turnContext")]
    turn_context_camel: MaybeMap<TurnContext>,
    token_count: MaybeMap<UsageInfo>,
    #[serde(rename = "tokenCount")]
    token_count_camel: MaybeMap<UsageInfo>,
    usage: MaybeMap<UsageInfo>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    model: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    timestamp: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    created_at: Option<String>,
    #[serde(rename = "createdAt", deserialize_with = "deserialize_lossy_string")]
    created_at_camel: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    id: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    message_id: Option<String>,
    #[serde(rename = "messageId", deserialize_with = "deserialize_lossy_string")]
    message_id_camel: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexPayload {
    #[serde(deserialize_with = "deserialize_lossy_string")]
    id: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    cwd: Option<String>,
    #[serde(deserialize_with = "deserialize_first_non_empty_trimmed_str_in_seq")]
    runtime_workspace_roots: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    parent_thread_id: Option<String>,
    source: MaybeMap<PayloadSource>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    model: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    reasoning_effort: Option<String>,
    #[serde(
        rename = "reasoningEffort",
        deserialize_with = "deserialize_lossy_string"
    )]
    reasoning_effort_camel: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    effort: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    r#type: Option<String>,
    info: MaybeMap<UsageInfo>,
}

impl CodexPayload {
    fn workspace(&self) -> Option<String> {
        self.cwd
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| self.runtime_workspace_roots.clone())
    }

    fn nested_parent_thread_id(&self) -> Option<&str> {
        self.source
            .0
            .as_ref()?
            .subagent
            .0
            .as_ref()?
            .thread_spawn
            .0
            .as_ref()?
            .parent_thread_id
            .as_deref()
    }

    fn effort_str(&self) -> Option<&str> {
        self.reasoning_effort
            .as_deref()
            .or(self.reasoning_effort_camel.as_deref())
            .or(self.effort.as_deref())
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PayloadSource {
    subagent: MaybeMap<PayloadSubagent>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PayloadSubagent {
    thread_spawn: MaybeMap<PayloadThreadSpawn>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PayloadThreadSpawn {
    #[serde(deserialize_with = "deserialize_lossy_string")]
    parent_thread_id: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TurnContext {
    #[serde(deserialize_with = "deserialize_lossy_string")]
    model: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    reasoning_effort: Option<String>,
    #[serde(
        rename = "reasoningEffort",
        deserialize_with = "deserialize_lossy_string"
    )]
    reasoning_effort_camel: Option<String>,
    #[serde(deserialize_with = "deserialize_lossy_string")]
    effort: Option<String>,
}

impl TurnContext {
    fn effort_str(&self) -> Option<&str> {
        self.reasoning_effort
            .as_deref()
            .or(self.reasoning_effort_camel.as_deref())
            .or(self.effort.as_deref())
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TotalTokenFields {
    #[serde(deserialize_with = "deserialize_lossy_u64")]
    total_tokens: Option<u64>,
    #[serde(rename = "totalTokens", deserialize_with = "deserialize_lossy_u64")]
    total_tokens_camel: Option<u64>,
    #[serde(deserialize_with = "deserialize_lossy_u64")]
    total: Option<u64>,
}

impl TotalTokenFields {
    fn explicit_total(&self) -> Option<u64> {
        self.total_tokens.or(self.total_tokens_camel).or(self.total)
    }
}

macro_rules! define_token_struct {
    (
        struct $name:ident {
            $($extra:tt)*
        }
    ) => {
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct $name {
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            input_tokens: Option<u64>,
            #[serde(rename = "inputTokens", deserialize_with = "deserialize_lossy_u64")]
            input_tokens_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            input: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            prompt_tokens: Option<u64>,
            #[serde(rename = "promptTokens", deserialize_with = "deserialize_lossy_u64")]
            prompt_tokens_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            cache_read: Option<u64>,
            #[serde(rename = "cacheRead", deserialize_with = "deserialize_lossy_u64")]
            cache_read_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            cached_input_tokens: Option<u64>,
            #[serde(rename = "cachedInputTokens", deserialize_with = "deserialize_lossy_u64")]
            cached_input_tokens_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            cache_write: Option<u64>,
            #[serde(rename = "cacheWrite", deserialize_with = "deserialize_lossy_u64")]
            cache_write_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            cache_write_input_tokens: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            output_tokens: Option<u64>,
            #[serde(rename = "outputTokens", deserialize_with = "deserialize_lossy_u64")]
            output_tokens_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            output: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            completion_tokens: Option<u64>,
            #[serde(rename = "completionTokens", deserialize_with = "deserialize_lossy_u64")]
            completion_tokens_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            reasoning_tokens: Option<u64>,
            #[serde(rename = "reasoningTokens", deserialize_with = "deserialize_lossy_u64")]
            reasoning_tokens_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            reasoning_output_tokens: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            total_tokens: Option<u64>,
            #[serde(rename = "totalTokens", deserialize_with = "deserialize_lossy_u64")]
            total_tokens_camel: Option<u64>,
            #[serde(deserialize_with = "deserialize_lossy_u64")]
            total: Option<u64>,
            $($extra)*
        }

        impl $name {
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
                compute_token_stats(
                    input_total,
                    cache_read,
                    cache_write,
                    output_total,
                    reasoning,
                    self.explicit_total(),
                )
            }
        }
    };
}

define_token_struct! {
    struct TokenFields {}
}

define_token_struct! {
    struct UsageInfo {
        total_token_usage: Option<MaybeMap<TotalTokenFields>>,
        last_token_usage: MaybeMap<TokenFields>,
        #[serde(rename = "lastTokenUsage")]
        last_token_usage_camel: MaybeMap<TokenFields>,
    }
}

struct MaybeMap<T>(Option<T>);

impl<T> Default for MaybeMap<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for MaybeMap<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct MaybeMapVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for MaybeMapVisitor<T> {
            type Value = MaybeMap<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an optional JSON object or ignored non-object value")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map))
                    .map(Some)
                    .map(MaybeMap)
            }

            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(MaybeMap(None))
            }

            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(MaybeMap(None))
            }

            fn visit_bool<E: serde::de::Error>(
                self,
                _: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(MaybeMap(None))
            }

            fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
                Ok(MaybeMap(None))
            }

            fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
                Ok(MaybeMap(None))
            }

            fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
                Ok(MaybeMap(None))
            }

            fn visit_str<E: serde::de::Error>(
                self,
                _: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(MaybeMap(None))
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(MaybeMap(None))
            }
        }

        deserializer.deserialize_any(MaybeMapVisitor(std::marker::PhantomData))
    }
}

struct LossyString(Option<String>);

impl<'de> Deserialize<'de> for LossyString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        deserialize_lossy_string(deserializer).map(LossyString)
    }
}

fn deserialize_lossy_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    struct LossyStringVisitor;

    impl<'de> Visitor<'de> for LossyStringVisitor {
        type Value = Option<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an optional string or ignored non-string JSON value")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
            Ok(Some(v.to_string()))
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

        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }

        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }

    deserializer.deserialize_any(LossyStringVisitor)
}

fn deserialize_lossy_u64<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error> {
    struct LossyU64Visitor;

    impl<'de> Visitor<'de> for LossyU64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an optional u64 or ignored non-u64 JSON value")
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

        fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }

        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }

    deserializer.deserialize_any(LossyU64Visitor)
}

fn deserialize_first_non_empty_trimmed_str_in_seq<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    struct FirstWorkspaceVisitor;

    impl<'de> Visitor<'de> for FirstWorkspaceVisitor {
        type Value = Option<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an optional array of workspace root strings")
        }

        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
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

        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut first = None;
            while first.is_none() {
                match seq.next_element::<LossyString>()? {
                    Some(LossyString(Some(s))) => {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            first = Some(trimmed.to_string());
                        }
                    }
                    Some(LossyString(None)) => {}
                    None => return Ok(None),
                }
            }
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(first)
        }

        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }

    deserializer.deserialize_any(FirstWorkspaceVisitor)
}

pub(crate) fn non_empty_trimmed_str(v: Option<&Value>) -> Option<String> {
    v?.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub(crate) fn string_at(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k)?.as_str().map(str::to_string))
}

pub(crate) fn u64_at(v: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| v.get(*k)?.as_u64())
}

fn compute_token_stats(
    input_total: u64,
    cache_read: u64,
    cache_write: u64,
    output_total: u64,
    reasoning: Option<u64>,
    explicit_total: Option<u64>,
) -> Result<TokenStats> {
    let component_total = input_total
        .checked_add(output_total)
        .context("token normalization overflow in input_total + output_total")?;
    let total_tokens = explicit_total.unwrap_or(component_total);
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
    compute_token_stats(
        input_total,
        cache_read,
        cache_write,
        output_total,
        reasoning,
        u64_at(v, &["total_tokens", "totalTokens", "total"]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metadata_uses_official_parents_not_fork_or_shared_session_id() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("sessions")).unwrap();
        std::fs::write(
            root.path().join("session_index.jsonl"),
            "{\"id\":\"child\",\"name\":\"Indexed\"}\n{\"id\":\"index-only\",\"name\":\"Absent\"}",
        )
        .unwrap();
        let values = [
            json!({"type":"session_meta","payload":{"id":"child","session_id":"shared","forked_from_id":"fork","parent_thread_id":"direct","source":{"subagent":{"thread_spawn":{"parent_thread_id":"official"}}}}}),
            json!({"type":"session_meta","payload":{"id":"child","parent_thread_id":null}}),
            json!({"type":"session_meta","payload":{"id":"fork-only","session_id":"shared","forked_from_id":"fork","parent_thread_id":42}}),
        ];
        std::fs::write(
            root.path().join("sessions/rollout-test.jsonl"),
            values
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let result = parse(root.path()).unwrap();
        assert!(result.records.is_empty());
        assert_eq!(result.session_metadata.len(), 2);
        let child = result
            .session_metadata
            .iter()
            .find(|m| m.key.id.0 == "child")
            .unwrap();
        assert_eq!(child.name.as_deref(), Some("Indexed"));
        assert_eq!(
            child
                .parent_candidates
                .keys()
                .map(|k| k.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["direct", "official"]
        );
        assert!(
            result
                .session_metadata
                .iter()
                .find(|m| m.key.id.0 == "fork-only")
                .unwrap()
                .parent_candidates
                .is_empty()
        );
    }

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

    #[test]
    fn session_meta_extracts_cwd_and_falls_back_to_runtime_workspace_roots() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let cwd_rows = [
            json!({"type":"session_meta","payload":{"id":"with-cwd","cwd":"  /work/project-a  "}}),
            json!({"type":"session_meta","payload":{"id":"with-cwd","cwd":"/work/should-not-overwrite"}}),
            json!({"type":"turn_context","payload":{"model":"m"}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}}}),
        ];
        std::fs::write(
            sessions.join("rollout-cwd.jsonl"),
            cwd_rows
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let fallback_rows = [
            json!({"type":"session_meta","payload":{"id":"with-roots","cwd":"  ","runtime_workspace_roots":["", 123, "  /work/project-b  ", "/work/other"]}}),
            json!({"type":"turn_context","payload":{"model":"m"}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":4,"output_tokens":1,"total_tokens":5}}}}),
        ];
        std::fs::write(
            sessions.join("rollout-roots.jsonl"),
            fallback_rows
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let result = parse(root.path()).unwrap();
        assert_eq!(result.records.len(), 2);
        let cwd_meta = result
            .session_metadata
            .iter()
            .find(|m| m.key.id.0 == "with-cwd")
            .unwrap();
        assert_eq!(cwd_meta.workspace.as_deref(), Some("/work/project-a"));
        let cwd_rec = result
            .records
            .iter()
            .find(|r| r.session_key.id.0 == "with-cwd")
            .unwrap();
        assert_eq!(cwd_rec.workspace.as_deref(), Some("/work/project-a"));
        let roots_meta = result
            .session_metadata
            .iter()
            .find(|m| m.key.id.0 == "with-roots")
            .unwrap();
        assert_eq!(roots_meta.workspace.as_deref(), Some("/work/project-b"));
        let roots_rec = result
            .records
            .iter()
            .find(|r| r.session_key.id.0 == "with-roots")
            .unwrap();
        assert_eq!(roots_rec.workspace.as_deref(), Some("/work/project-b"));
    }

    #[test]
    fn parallel_parsing_matches_sequential_order_and_counts() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let nested = sessions.join("2026/11");
        std::fs::create_dir_all(&nested).unwrap();

        let mut index_lines = Vec::new();
        let mut expected_paths = Vec::new();
        let big_blob = "x".repeat(8192);

        for i in 0..12usize {
            let sid = format!("sess-{i:02}");
            index_lines.push(json!({"sessionId": sid, "title": format!("Title {i}")}).to_string());
            let dir = if i % 2 == 0 { &sessions } else { &nested };
            let file_path = dir.join(format!("rollout-{i:02}.jsonl"));
            expected_paths.push(file_path.clone());

            let mut lines = vec![
                json!({
                    "type": "session_meta",
                    "payload": {
                        "id": sid,
                        "cwd": if i % 3 == 0 { format!("  /ws/{i}  ") } else { String::new() },
                        "runtime_workspace_roots": ["", 42, format!("/fallback/{i}")],
                        "parent_thread_id": if i % 4 == 0 { json!(42) } else { json!(format!("p-{i}")) },
                        "base_instructions": big_blob
                    }
                })
                .to_string(),
                "null".to_string(),
                json!({
                    "type": "turn_context",
                    "payload": {
                        "model": format!("model-{i}"),
                        "reasoningEffort": "high"
                    }
                })
                .to_string(),
                json!({
                    "type": "response_item",
                    "payload": {
                        "content": [{"type": "output_text", "text": big_blob}],
                        "output": big_blob
                    }
                })
                .to_string(),
                json!({
                    "type": "event_msg",
                    "timestamp": if i % 5 == 0 { "not-a-timestamp" } else { "2026-11-20T00:00:00Z" },
                    "id": format!("msg-{i}"),
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "total_token_usage": {"total_tokens": 10 + i as u64},
                            "last_token_usage": {
                                "input_tokens": 6 + i as u64,
                                "output_tokens": 4,
                                "cached_input_tokens": 1,
                                "reasoning_output_tokens": 2,
                                "total_tokens": 10 + i as u64
                            }
                        }
                    }
                })
                .to_string(),
            ];

            if i % 3 == 1 {
                // Stale cumulative delta -> CodexNonPositiveDelta
                lines.push(
                    json!({
                        "type": "event_msg",
                        "payload": {
                            "type": "token_count",
                            "info": {
                                "total_token_usage": {"total_tokens": 10 + i as u64}
                            }
                        }
                    })
                    .to_string(),
                );
            }
            if i % 4 == 2 {
                // Malformed JSON line -> CodexMalformedLine
                lines.push("{broken-json".to_string());
            }
            if i % 6 == 3 {
                // Non-native event with wrong-type total_tokens -> ignored safely as None
                lines.push(
                    json!({
                        "usage": {
                            "input_tokens": 2,
                            "output_tokens": 3,
                            "total_tokens": "bad"
                        }
                    })
                    .to_string(),
                );
            }
            if i == 11 {
                // Overflow -> CodexTokenOverflow
                lines.push(
                    json!({
                        "usage": {
                            "input_tokens": u64::MAX,
                            "output_tokens": 1
                        }
                    })
                    .to_string(),
                );
            }

            std::fs::write(&file_path, lines.join("\n")).unwrap();
        }
        std::fs::write(
            root.path().join("session_index.jsonl"),
            index_lines.join("\n"),
        )
        .unwrap();
        expected_paths.sort();

        // Sequential ground truth over sorted paths
        let mut seq_summary = ScanSummary {
            client: Some(Client::Codex),
            ..Default::default()
        };
        let names = read_index(root.path(), &mut seq_summary);
        let mut seq_records = Vec::new();
        let mut seq_meta = Vec::new();
        let mut seq_diags = Vec::new();
        for p in &expected_paths {
            let out = parse_file(p, &names).unwrap();
            merge_output(
                out,
                &mut seq_records,
                &mut seq_meta,
                &mut seq_diags,
                &mut seq_summary,
            );
        }

        // Parallel with explicit 4 workers and full parse() entrypoint
        let par_outputs = parse_files_parallel(&expected_paths, &names, 4).unwrap();
        let mut par_records = Vec::new();
        let mut par_meta = Vec::new();
        let mut par_diags = Vec::new();
        let mut par_summary = ScanSummary {
            client: Some(Client::Codex),
            ..Default::default()
        };
        let _ = read_index(root.path(), &mut par_summary);
        for out in par_outputs {
            merge_output(
                out,
                &mut par_records,
                &mut par_meta,
                &mut par_diags,
                &mut par_summary,
            );
        }

        let run1 = parse(root.path()).unwrap();
        let run2 = parse(root.path()).unwrap();

        for res in [
            (
                &run1.records,
                &run1.session_metadata,
                &run1.diagnostics,
                &run1.summary,
            ),
            (
                &run2.records,
                &run2.session_metadata,
                &run2.diagnostics,
                &run2.summary,
            ),
            (&par_records, &par_meta, &par_diags, &par_summary),
        ] {
            assert_eq!(res.3.files_scanned, seq_summary.files_scanned);
            assert_eq!(res.3.lines_read, seq_summary.lines_read);
            assert_eq!(res.3.records_emitted, seq_summary.records_emitted);
            assert_eq!(res.3.records_skipped, seq_summary.records_skipped);

            assert_eq!(res.0.len(), seq_records.len());
            for (got, exp) in res.0.iter().zip(seq_records.iter()) {
                assert_eq!(got.session_key, exp.session_key);
                assert_eq!(got.message_id, exp.message_id);
                assert_eq!(got.source_path, exp.source_path);
                assert_eq!(got.source_line, exp.source_line);
                assert_eq!(got.started_at, exp.started_at);
                assert_eq!(got.session_name, exp.session_name);
                assert_eq!(got.workspace, exp.workspace);
                assert_eq!(got.model, exp.model);
                assert_eq!(got.reasoning_effort, exp.reasoning_effort);
                assert_eq!(got.tokens, exp.tokens);
            }

            assert_eq!(res.1.len(), seq_meta.len());
            for (got, exp) in res.1.iter().zip(seq_meta.iter()) {
                assert_eq!(got.key, exp.key);
                assert_eq!(got.name, exp.name);
                assert_eq!(got.workspace, exp.workspace);
                assert_eq!(got.parent_candidates, exp.parent_candidates);
                assert_eq!(got.source_path, exp.source_path);
                assert_eq!(got.source_line, exp.source_line);
            }

            assert_eq!(res.2.len(), seq_diags.len());
            for (got, exp) in res.2.iter().zip(seq_diags.iter()) {
                assert_eq!(got.code, exp.code);
                assert_eq!(got.path, exp.path);
                assert_eq!(got.line, exp.line);
            }
        }
    }
}
