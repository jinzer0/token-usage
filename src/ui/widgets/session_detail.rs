use crate::{
    aggregate::{ModelStats, SessionStats},
    domain::{Client, ReasoningEffort, TokenStats},
    ui::{
        app::{App, DetailScope, PaneFocus},
        format,
        layout::LayoutMode,
        theme::Theme,
    },
};
use chrono::Local;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme, mode: LayoutMode) {
    let focused = app.focus == PaneFocus::Details;
    let border_style = if focused {
        theme.border_focused()
    } else {
        theme.border()
    };
    let viewport_height = area.height.saturating_sub(2) as usize;

    if app.snapshot.sessions.is_empty() {
        app.set_detail_metrics(0, viewport_height);
        let text = vec![
            Line::styled("No usage sessions found.", theme.title()),
            Line::raw(""),
            Line::styled("Local stores: Codex, GJC, OpenCode.", theme.muted_text()),
            Line::styled("Press r to refresh.", theme.muted_text()),
        ];
        frame.render_widget(
            Paragraph::new(text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style),
            ),
            area,
        );
        return;
    }

    let Some(session_idx) = app.selected_index() else {
        app.set_detail_metrics(0, viewport_height);
        frame.render_widget(
            Paragraph::new(
                "No matching sessions. Press Esc to clear search or c/f to reset filters.",
            )
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style),
            ),
            area,
        );
        return;
    };
    let session = &app.snapshot.sessions[session_idx];

    let raw_title = session.name.as_deref().unwrap_or(&session.key.id.0);
    let max_title_width = area.width.saturating_sub(4) as usize;
    let title = if mode == LayoutMode::Narrow {
        if let Some(pos) = app.selected_filtered_position() {
            if !app.filtered_sessions.is_empty() {
                let prefix = format!("[{}/{}] ", pos + 1, app.filtered_sessions.len());
                let name_budget = max_title_width.saturating_sub(format::display_width(&prefix));
                format!(
                    "{prefix}{}",
                    format::truncate_ellipsis(raw_title, name_budget)
                )
            } else {
                format::truncate_ellipsis(raw_title, max_title_width)
            }
        } else {
            format::truncate_ellipsis(raw_title, max_title_width)
        }
    } else {
        format::truncate_ellipsis(raw_title, max_title_width)
    };

    let inner_width = area.width.saturating_sub(2);
    let periods = if app.detail_scope == DetailScope::All {
        app.session_period_totals(session_idx)
    } else {
        None
    };
    let scoped_all = if app.detail_scope == DetailScope::All
        && app.period_filter != crate::ui::app::PeriodFilter::All
    {
        app.filtered_session_detail(session_idx)
    } else {
        None
    };
    let lines = match &scoped_all {
        Some((tokens, models, record_count)) => detail_lines_with_data(
            session,
            app.period_filter.label().to_string(),
            tokens,
            models.as_slice(),
            *record_count,
            periods,
            theme,
            app.breakdown,
            mode,
            inner_width as usize,
        ),
        None => detail_lines(
            session,
            app.detail_scope,
            periods,
            theme,
            app.breakdown,
            mode,
            inner_width as usize,
        ),
    };
    let total_lines = wrapped_line_count(&lines, inner_width as usize);
    app.set_detail_metrics(total_lines, viewport_height);
    let paragraph = Paragraph::new(lines)
        .scroll((app.detail_scroll.min(u16::MAX as usize) as u16, 0))
        .wrap(Wrap { trim: false });

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    if viewport_height > 0 && total_lines > viewport_height {
        let start = (app.detail_scroll + 1).min(total_lines);
        let end = app
            .detail_scroll
            .saturating_add(viewport_height)
            .min(total_lines);
        let arrow = match (app.detail_scroll > 0, end < total_lines) {
            (true, true) => " ↕",
            (false, true) => " ↓",
            (true, false) => " ↑",
            (false, false) => "",
        };
        let indicator = format!(" L{start}-{end}/{total_lines}{arrow} ");
        block = block.title_bottom(
            Line::from(indicator)
                .right_aligned()
                .style(theme.muted_text()),
        );
    }

    frame.render_widget(paragraph.block(block), area);
}

fn detail_lines(
    session: &SessionStats,
    scope: DetailScope,
    periods: Option<crate::ui::app::SessionPeriodTotals>,
    theme: &Theme,
    breakdown: bool,
    mode: LayoutMode,
    width: usize,
) -> Vec<Line<'static>> {
    let (tokens, models, record_count) = match scope {
        DetailScope::All => (
            &session.tokens,
            session.models.as_slice(),
            session.record_count,
        ),
        scope => {
            let day = match scope {
                DetailScope::Day(day) => Some(day),
                _ => None,
            };
            let Some(bucket) = session.daily.iter().find(|bucket| bucket.day == day) else {
                return vec![
                    Line::styled(scope.label(), theme.title()),
                    Line::raw("No usage records in this date range."),
                ];
            };
            (
                &bucket.tokens,
                bucket.models.as_slice(),
                bucket.record_count,
            )
        }
    };

    detail_lines_with_data(
        session,
        scope.label(),
        tokens,
        models,
        record_count,
        periods,
        theme,
        breakdown,
        mode,
        width,
    )
}

#[allow(clippy::too_many_arguments)]
fn detail_lines_with_data(
    session: &SessionStats,
    scope_label: String,
    tokens: &TokenStats,
    models: &[ModelStats],
    record_count: u64,
    periods: Option<crate::ui::app::SessionPeriodTotals>,
    theme: &Theme,
    breakdown: bool,
    mode: LayoutMode,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let client = match session.key.client {
        Client::Codex => "CODEX",
        Client::Gjc => "GJC",
        Client::OpenCode => "OPENCODE",
    };
    lines.push(Line::from(vec![
        Span::styled(
            client.to_string(),
            match session.key.client {
                Client::Codex => theme.client_codex(),
                Client::Gjc => theme.client_gjc(),
                Client::OpenCode => theme.client_opencode(),
            },
        ),
        Span::raw(" "),
        Span::styled(
            format::pad_left(
                &format::token_count(tokens.total_tokens),
                width.saturating_sub(client.len() + 1),
            ),
            theme.number(),
        ),
    ]));

    let rec_label = if record_count == 1 {
        "1 record".to_string()
    } else {
        format!("{record_count} records")
    };
    lines.push(Line::styled(
        format!("{scope_label} · {rec_label}"),
        theme.muted_text(),
    ));

    if let Some(name) = &session.name
        && name != &session.key.id.0
    {
        let max_id_len = width.saturating_sub(3).max(8);
        let truncated_id = format::truncate_ellipsis(&session.key.id.0, max_id_len);
        lines.push(Line::styled(
            format!("ID {truncated_id}"),
            theme.muted_text(),
        ));
    }

    if let Some(workspace) = &session.workspace {
        let shortened = format::shorten_home(workspace);
        let max_ws_len = width.saturating_sub(10).max(8);
        let truncated_ws = format::truncate_ellipsis(&shortened, max_ws_len);
        lines.push(Line::from(vec![
            Span::styled("Workspace ", theme.muted_text()),
            Span::styled(truncated_ws, theme.primary_text()),
        ]));
    }

    let time_line = match (session.last_used_at, session.started_at) {
        (Some(last), Some(started)) => Some(format!(
            "Last used {} · Started {}",
            last.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
            started.with_timezone(&Local).format("%Y-%m-%d %H:%M")
        )),
        (Some(last), None) => Some(format!(
            "Last used {}",
            last.with_timezone(&Local).format("%Y-%m-%d %H:%M")
        )),
        (None, Some(started)) => Some(format!(
            "Started {}",
            started.with_timezone(&Local).format("%Y-%m-%d %H:%M")
        )),
        (None, None) => None,
    };
    if let Some(ts) = time_line {
        lines.push(Line::styled(ts, theme.muted_text()));
    }

    let mut total_spans = vec![Span::styled(
        format!("Total {}", tokens.total_tokens),
        theme.number(),
    )];
    if let Some(hit) = format::cache_hit_rate(tokens.cache_read, tokens.input_total) {
        total_spans.push(Span::styled(
            format!("  ·  Cache hit {hit}"),
            theme.muted_text(),
        ));
    }
    lines.push(Line::from(total_spans));

    if let Some(p) = periods {
        let mut period_text = format!(
            "today {} · 7d {} · 30d {}",
            format::token_count(p.today),
            format::token_count(p.seven_days),
            format::token_count(p.thirty_days),
        );
        if let Some((days, custom_total)) = p.custom
            && !matches!(days, 1 | 7 | 30)
        {
            period_text.push_str(&format!(" · {days}d {}", format::token_count(custom_total)));
        }
        period_text.push_str(&format!(" · all {}", format::token_count(p.all)));
        lines.push(Line::from(vec![
            Span::styled("Periods  ", theme.muted_text()),
            Span::styled(period_text, theme.primary_text()),
        ]));
    }

    if breakdown {
        push_exact_components(&mut lines, tokens, theme);
    }

    let divider_len = width.clamp(8, 46);
    lines.push(Line::styled("─".repeat(divider_len), theme.border()));

    for (idx, model) in models.iter().enumerate() {
        if idx > 0 {
            lines.push(Line::raw(""));
        }
        push_model(
            lines.as_mut(),
            model,
            tokens.total_tokens,
            theme,
            breakdown,
            mode,
            width,
        );
    }
    if models.is_empty() {
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
        Span::raw(" ".repeat(
            width.saturating_sub(format::display_width(&name) + total.len() + pct.len() + 2),
        )),
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
    if breakdown {
        lines.push(Line::styled(
            format!("  Model exact total {}", model.tokens.total_tokens),
            theme.number(),
        ));
        push_exact_components(lines, &model.tokens, theme);
        for effort in &model.efforts {
            lines.push(Line::styled(
                format!(
                    "  Effort {} exact total {}",
                    effort_label(&effort.effort),
                    effort.tokens.total_tokens
                ),
                theme.number(),
            ));
            push_exact_components(lines, &effort.tokens, theme);
        }
    }
}

fn push_exact_components(lines: &mut Vec<Line<'static>>, tokens: &TokenStats, theme: &Theme) {
    for (indent, label, value, style) in [
        ("  ", "INPUT", tokens.input_total, theme.primary_text()),
        (
            "    ",
            "Uncached",
            tokens.input_uncached,
            theme.muted_text(),
        ),
        ("    ", "Cache read", tokens.cache_read, theme.muted_text()),
        (
            "    ",
            "Cache write",
            tokens.cache_write,
            theme.muted_text(),
        ),
        ("  ", "OUTPUT", tokens.output_total, theme.primary_text()),
    ] {
        lines.push(Line::styled(format!("{indent}{label} {value}"), style));
    }
    lines.push(Line::styled(
        format!(
            "  REASONING {}{}",
            tokens.reasoning_known,
            if tokens.reasoning_has_unknown {
                "+ (some unknown)"
            } else {
                ""
            }
        ),
        theme.reasoning(),
    ));
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

fn wrapped_line_count(lines: &[Line<'_>], width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
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
            workspace: None,
            started_at: None,
            last_used_at: None,
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
            daily: vec![],
        };
        let lines = detail_lines(
            &session,
            DetailScope::All,
            None,
            &Theme::default(),
            false,
            LayoutMode::Wide,
            80,
        );
        let text = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("gpt"));
        assert!(text.contains("xhigh"));
        assert!(text.contains("reasoning"));
        assert!(text.contains("1 record"));
        assert!(text.contains("ID s"));
    }

    #[test]
    fn daily_detail_uses_scoped_models_denominators_and_breakdown() {
        let snapshot = crate::ui::app::daily_test_snapshot([true; 3]);
        let session = &snapshot.sessions[0];
        let day = session.daily[0].day.unwrap();
        let text = |scope, breakdown, mode, width| {
            detail_lines(
                session,
                scope,
                None,
                &Theme::default(),
                breakdown,
                mode,
                width,
            )
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n")
        };
        let all = text(DetailScope::All, false, LayoutMode::Wide, 100);
        assert!(all.contains("day-old") && all.contains("day-new") && all.contains("day-unknown"));
        assert!(all.contains("27%"));
        assert!(all.contains("3 records"));
        assert!(all.contains("Cache hit"));
        let selected = text(DetailScope::Day(day), true, LayoutMode::Wide, 100);
        assert!(
            selected.contains("Local") && selected.contains("day-new") && selected.contains("300")
        );
        assert!(
            selected.contains("100%") && selected.contains("INPUT") && selected.contains("20+")
        );
        assert!(
            !selected.contains("day-old")
                && !selected.contains("day-unknown")
                && !selected.contains("27%")
        );
        let undated = text(DetailScope::Undated, true, LayoutMode::Narrow, 38);
        assert!(
            undated.contains("Unknown date")
                && undated.contains("day-unknown")
                && undated.contains("0+")
        );
        assert!(!undated.contains("day-new") && !undated.contains("day-old"));
        let missing = text(
            DetailScope::Day(day + chrono::Duration::days(10)),
            false,
            LayoutMode::Wide,
            100,
        );
        assert!(missing.contains("No usage records"));
        assert!(!missing.contains("day-new"));
    }

    #[test]
    fn narrow_title_shows_position_and_scroll_indicator_appears_when_overflowing() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = App::new(crate::ui::app::daily_test_snapshot([true; 3]));
        app.breakdown = true;
        let mut terminal = Terminal::new(TestBackend::new(44, 10)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &mut app,
                    &Theme::default(),
                    LayoutMode::Narrow,
                )
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("[1/1]"), "missing narrow position: {text}");
        assert!(text.contains("L1-"), "missing scroll indicator: {text}");
    }
}
