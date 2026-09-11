use crate::domain::Client;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub client: Client,
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub path: Option<PathBuf>,
    pub line: Option<u64>,
}

impl Diagnostic {
    pub fn warning(client: Client, code: impl Into<String>, message: impl Into<String>, path: Option<PathBuf>, line: Option<u64>) -> Self {
        Self { client, severity: Severity::Warning, code: code.into(), message: message.into(), path, line }
    }

    pub fn error(client: Client, code: impl Into<String>, message: impl Into<String>, path: Option<PathBuf>, line: Option<u64>) -> Self {
        Self { client, severity: Severity::Error, code: code.into(), message: message.into(), path, line }
    }
}
