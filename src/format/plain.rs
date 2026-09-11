use crate::aggregate::{AggregateSnapshot, SessionStats};

pub fn summary(snapshot: &AggregateSnapshot, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("token-usage summary\n");
    out.push_str(&format!(
        "files={} records={} skipped={} warnings={} errors={}\n",
        snapshot.source_counts.files_scanned,
        snapshot.source_counts.records_emitted,
        snapshot.source_counts.records_skipped,
        snapshot.source_counts.warnings,
        snapshot.source_counts.errors
    ));
    if snapshot.sessions.is_empty() {
        out.push_str("No usage records found.\n");
        return out;
    }
    out.push_str("client session total input output reasoning\n");
    for session in &snapshot.sessions {
        out.push_str(&format!(
            "{} {} {} {} {} {}\n",
            session.key.client,
            display_name(session),
            session.tokens.total_tokens,
            session.tokens.input_total,
            session.tokens.output_total,
            reasoning(&session.tokens)
        ));
        if verbose {
            out.push_str(&format!(
                "  cache_read={} cache_write={} input_uncached={} records={}\n",
                session.tokens.cache_read,
                session.tokens.cache_write,
                session.tokens.input_uncached,
                session.record_count
            ));
        }
    }
    out
}

pub fn detail(session: &SessionStats, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("session {}\n", session.key.qualified()));
    if let Some(name) = &session.name {
        out.push_str(&format!("name {}\n", name));
    }
    if let Some(parent) = &session.parent {
        out.push_str(&format!("parent {}\n", parent.qualified()));
    }
    out.push_str(&format!(
        "total {} input {} output {} reasoning {}\n",
        session.tokens.total_tokens,
        session.tokens.input_total,
        session.tokens.output_total,
        reasoning(&session.tokens)
    ));
    for model in &session.models {
        out.push_str(&format!(
            "  model {} total {}\n",
            model.model, model.tokens.total_tokens
        ));
        for effort in &model.efforts {
            let label = effort
                .effort
                .as_ref()
                .map(|e| e.label())
                .unwrap_or_else(|| "none".into());
            out.push_str(&format!(
                "    effort {} total {}\n",
                label, effort.tokens.total_tokens
            ));
            if verbose {
                out.push_str(&format!(
                    "      input={} output={} cache_read={} reasoning={} records={}\n",
                    effort.tokens.input_total,
                    effort.tokens.output_total,
                    effort.tokens.cache_read,
                    reasoning(&effort.tokens),
                    effort.record_count
                ));
            }
        }
    }
    out
}

fn display_name(session: &SessionStats) -> String {
    session
        .name
        .clone()
        .unwrap_or_else(|| session.key.id.0.clone())
}

fn reasoning(tokens: &crate::domain::TokenStats) -> String {
    if tokens.reasoning_has_unknown {
        format!("{}+", tokens.reasoning_known)
    } else {
        tokens.reasoning_known.to_string()
    }
}
