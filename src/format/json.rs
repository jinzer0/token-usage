use crate::aggregate::AggregateSnapshot;
use anyhow::Result;

pub fn render(snapshot: &AggregateSnapshot) -> Result<String> {
    Ok(serde_json::to_string_pretty(snapshot)?)
}
