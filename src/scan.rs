use crate::{
    diagnostics::Diagnostic,
    domain::{Client, SessionKey, UsageRecord},
};
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug)]
pub struct SessionMetadata {
    pub key: SessionKey,
    pub parent_candidates: BTreeMap<SessionKey, (PathBuf, Option<u64>)>,
    pub name: Option<String>,
    pub source_path: PathBuf,
    pub source_line: Option<u64>,
}

impl SessionMetadata {
    pub fn add_parent(&mut self, key: SessionKey, path: PathBuf, line: Option<u64>) {
        let location = (path, line);
        self.parent_candidates
            .entry(key)
            .and_modify(|existing| {
                if location < *existing {
                    *existing = location.clone();
                }
            })
            .or_insert(location);
    }
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct ScanSummary {
    pub client: Option<Client>,
    pub roots_scanned: Vec<PathBuf>,
    pub missing_roots: Vec<PathBuf>,
    pub empty_roots: Vec<PathBuf>,
    pub files_scanned: u64,
    pub lines_read: u64,
    pub records_emitted: u64,
    pub records_skipped: u64,
    pub records_time_filtered: u64,
    pub records_missing_timestamp_filtered: u64,
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct SourceCounts {
    pub roots_scanned: Vec<PathBuf>,
    pub missing_roots: Vec<PathBuf>,
    pub empty_roots: Vec<PathBuf>,
    pub files_scanned: u64,
    pub lines_read: u64,
    pub records_emitted: u64,
    pub records_skipped: u64,
    pub records_time_filtered: u64,
    pub records_missing_timestamp_filtered: u64,
    pub warnings: u64,
    pub errors: u64,
}

#[derive(Clone, Debug)]
pub struct ParseResult {
    pub records: Vec<UsageRecord>,
    pub session_metadata: Vec<SessionMetadata>,
    pub diagnostics: Vec<Diagnostic>,
    pub summary: ScanSummary,
}

impl ParseResult {
    pub fn empty(client: Client, root: PathBuf) -> Self {
        let mut summary = ScanSummary {
            client: Some(client),
            ..Default::default()
        };
        if root.exists() {
            summary.roots_scanned.push(root);
        } else {
            summary.missing_roots.push(root);
        }
        Self {
            records: Vec::new(),
            session_metadata: Vec::new(),
            diagnostics: Vec::new(),
            summary,
        }
    }
}
