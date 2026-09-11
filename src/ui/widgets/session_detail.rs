use crate::{
    aggregate::{ModelStats, SessionStats},
    domain::{Client, ReasoningEffort, TokenStats},
    ui::{app::App, format, layout::LayoutMode, theme::Theme},
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, mode: LayoutMode) {
    if app.snapshot.sessions.is_empty() {
        let text = vec![
            Line::styled("No usage sessions found.", theme.title()),
            Line::raw(""),
            Line::styled(
                "Scanned Codex and GJC local session stores.",
                theme.muted_text(),
            ),
            Line::styled("Press r to refresh.", theme.muted_text()),
        ];
        frame.render_widget(
            Paragraph::new(text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.border()),
            ),
            area,
        );
        return;
    }

    let Some(session) = app.selected_session() else {
        frame.render_widget(
            Paragraph::new("No matching sessions. Press Esc to clear search.").block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.border()),
            ),
            area,
        );
        return;
    };

    let title = format::truncate_ellipsis(
        session.name.as_deref().unwrap_or(&session.key.id.0),
        area.width.saturating_sub(4) as usize,
    );
    let lines = detail_lines(
        session,
        theme,
        app.breakdown,
        mode,
        area.width.saturating_sub(2) as usize,
    );
    let paragraph = Paragraph::new(lines)
        .scroll((app.detail_scroll as u16, 0))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border())
                .title(title),
        );
    frame.render_widget(paragraph, area);
}

fn detail_lines(
    session: &SessionStats,
    theme: &Theme,
    breakdown: bool,
    mode: LayoutMode,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let client = match session.key.client {
        Client::Codex => "CODEX",
        Client::Gjc => "GJC",
    };
    lines.push(Line::from(vec![
        Span::styled(
            client.to_string(),
            match session.key.client {
                Client::Codex => theme.client_codex(),
                Client::Gjc => theme.client_gjc(),
            },
        ),
        Span::raw(" "),
        Span::styled(
            format::pad_left(
                &format::token_count(session.tokens.total_tokens),
                width.saturating_sub(client.len() + 1),
            ),
            theme.number(),
        ),
    ]));
    lines.push(Line::raw(""));

    for (idx, model) in session.models.iter().enumerate() {
        if idx > 0 {
            lines.push(Line::raw(""));
        }
        push_model(
            lines.as_mut(),
            model,
            session.tokens.total_tokens,
            theme,
            breakdown,
            mode,
            width,
        );
    }
    if session.models.is_empty() {
        lines.push(Line::styled(
            "No model usage records in this session.",
            theme.muted_text(),
        ));
    }
    lines
}

fn push_model(
    lines: &mut Vec<Line<'static>>,
    model: &ModelStats,
    session_total: u64,
    theme: &Theme,
    breakdown: bool,
    mode: LayoutMode,
    width: usize,
) {
    let pct = format::percentage(model.tokens.total_tokens, session_total);
    let total = format::token_count(model.tokens.total_tokens);
    let left_width = width.saturating_sub(total.len() + pct.len() + 3).max(1);
    let name = format::truncate_ellipsis(&model.model, left_width);
    lines.push(Line::from(vec![
        Span::styled(name.clone(), theme.model_name()),
        Span::raw(
            " ".repeat(width.saturating_sub(name.chars().count() + total.len() + pct.len() + 2)),
        ),
        Span::styled(total, theme.number()),
        Span::raw(" "),
        Span::styled(pct, theme.percentage()),
    ]));

    let bar_width = width.min(46).saturating_sub(2).max(8);
    let bar = format::usage_bar(model.tokens.total_tokens, session_total, bar_width);
    lines.push(Line::from(vec![Span::styled(bar, theme.bar_full())]));

    if breakdown && mode == LayoutMode::Wide && width >= 72 {
        lines.push(Line::styled(
            "  EFFORT        TOTAL    INPUT  CACHE-R  CACHE-W   OUTPUT  REASONING",
            theme.muted_text(),
        ));
        for effort in &model.efforts {
            let label = effort_label(&effort.effort);
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("{:<10}", format::truncate_ellipsis(&label, 10)),
                    theme.primary_text(),
                ),
                Span::styled(
                    format!("{:>8}", format::token_count(effort.tokens.total_tokens)),
                    theme.number(),
                ),
                Span::styled(
                    format!("{:>9}", format::token_count(effort.tokens.input_total)),
                    theme.primary_text(),
                ),
                Span::styled(
                    format!("{:>9}", format::token_count(effort.tokens.cache_read)),
                    theme.primary_text(),
                ),
                Span::styled(
                    format!("{:>9}", format::token_count(effort.tokens.cache_write)),
                    theme.primary_text(),
                ),
                Span::styled(
                    format!("{:>9}", format::token_count(effort.tokens.output_total)),
                    theme.primary_text(),
                ),
                Span::styled(
                    format!("{:>11}", reasoning(&effort.tokens)),
                    theme.reasoning(),
                ),
            ]));
        }
    } else {
        for effort in &model.efforts {
            push_effort(
                lines,
                &effort.effort,
                &effort.tokens,
                model.tokens.total_tokens,
                theme,
                breakdown,
                width,
            );
        }
    }
}

fn push_effort(
    lines: &mut Vec<Line<'static>>,
    effort: &Option<ReasoningEffort>,
    tokens: &TokenStats,
    model_total: u64,
    theme: &Theme,
    breakdown: bool,
    width: usize,
) {
    let label = effort_label(effort);
    let total = format::token_count(tokens.total_tokens);
    let pct = format::percentage(tokens.total_tokens, model_total);
    let reason = reasoning(tokens);
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{:<10}", format::truncate_ellipsis(&label, 10)),
            theme.primary_text(),
        ),
        Span::styled(format!("{:>8}", total), theme.number()),
        Span::styled(format!("{:>6}", pct), theme.percentage()),
        Span::raw("   reasoning "),
        Span::styled(format!("{:>8}", reason), theme.reasoning()),
    ]));
    if breakdown {
        let meta = format!(
            "    in {} · cache-r {} · cache-w {} · out {}",
            format::token_count(tokens.input_total),
            format::token_count(tokens.cache_read),
            format::token_count(tokens.cache_write),
            format::token_count(tokens.output_total)
        );
        lines.push(Line::styled(
            format::truncate_ellipsis(&meta, width),
            theme.muted_text(),
        ));
    }
}

fn effort_label(effort: &Option<ReasoningEffort>) -> String {
    effort
        .as_ref()
        .map(|e| e.label())
        .unwrap_or_else(|| "none".into())
}

fn reasoning(tokens: &TokenStats) -> String {
    format::reasoning(tokens.reasoning_known, tokens.reasoning_has_unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::EffortStats;

    #[test]
    fn partial_reasoning_has_marker() {
        let tokens = TokenStats {
            reasoning_known: 91_000,
            reasoning_has_unknown: true,
            ..Default::default()
        };
        assert_eq!(reasoning(&tokens), "91K+");
    }

    #[test]
    fn unknown_effort_renders_as_none() {
        assert_eq!(effort_label(&None), "none");
    }

    #[test]
    fn detail_contains_model_and_effort_without_plain_formatter() {
        let session = SessionStats {
            key: crate::domain::SessionKey {
                client: Client::Gjc,
                id: crate::domain::SessionId("s".into()),
            },
            parent: None,
            name: Some("session".into()),
            started_at: None,
            tokens: TokenStats {
                total_tokens: 100,
                ..Default::default()
            },
            models: vec![ModelStats {
                model: "gpt".into(),
                tokens: TokenStats {
                    total_tokens: 100,
                    ..Default::default()
                },
                record_count: 1,
                efforts: vec![EffortStats {
                    effort: Some(ReasoningEffort::Custom("xhigh".into())),
                    tokens: TokenStats {
                        total_tokens: 100,
                        reasoning_known: 25,
                        ..Default::default()
                    },
                    record_count: 1,
                }],
            }],
            record_count: 1,
        };
        let lines = detail_lines(&session, &Theme::default(), false, LayoutMode::Wide, 80);
        let text = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("gpt"));
        assert!(text.contains("xhigh"));
        assert!(text.contains("reasoning"));
    }
}
