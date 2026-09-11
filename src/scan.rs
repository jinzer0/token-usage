use crate::{
    diagnostics::{Diagnostic, Severity},
    domain::{Client, UsageRecord},
};
use serde::Serialize;
use std::path::PathBuf;

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
    pub warnings: u64,
    pub errors: u64,
}

impl SourceCounts {
    pub fn merge_summary(&mut self, summary: &ScanSummary) {
        self.roots_scanned.extend(summary.roots_scanned.clone());
        self.missing_roots.extend(summary.missing_roots.clone());
        self.empty_roots.extend(summary.empty_roots.clone());
        self.files_scanned += summary.files_scanned;
        self.lines_read += summary.lines_read;
        self.records_emitted += summary.records_emitted;
        self.records_skipped += summary.records_skipped;
    }

    pub fn merge_diagnostics(&mut self, diagnostics: &[Diagnostic]) {
        for diagnostic in diagnostics {
            match diagnostic.severity {
                Severity::Warning => self.warnings += 1,
                Severity::Error => self.errors += 1,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParseResult {
    pub records: Vec<UsageRecord>,
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
            diagnostics: Vec::new(),
            summary,
        }
    }
}
