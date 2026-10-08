pub mod codex;
pub mod gjc;
pub mod opencode;

use crate::{domain::Client, scan::ParseResult};
use anyhow::Result;
use std::path::Path;

pub trait UsageSource {
    fn client(&self) -> Client;
    fn parse_root(&self, root: &Path) -> Result<ParseResult>;
}
