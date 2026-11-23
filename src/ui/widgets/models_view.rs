use crate::{
    domain::Client,
    ui::{
        app::{App, ModelAggregateRow, PaneFocus},
        format,
        layout::AppLayout,
        theme::Theme,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Sparkline, Wrap},
};

pub fn render(frame: &mut Frame<'_>, layout: AppLayout, app: &mut App, theme: &Theme) {
    if let Some(chart_area) = layout.chart {
        render_trend_chart(frame, chart_area, app, theme);
    }
    if let Some(models_area) = layout.models {
        render_models_list(frame, models_area, app, theme);
    }
    render_model_detail(frame, layout.detail, app, theme);
}

fn render_trend_chart(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let scope = format!(
        "{} / {}",
        app.client_filter.label(),
        app.period_filter.label()
    );
    if app.trend_points.is_empty() {
        let title = format!("Daily Trend · {scope}");
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No dated usage in current filter.",
                theme.muted_text(),
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.border())
                    .title(title),
            ),
            area,
        );
        return;
    }

    let start = app.trend_points.len().saturating_sub(inner_width);
    let visible = &app.trend_points[start..];
    let values: Vec<u64> = visible.iter().map(|p| p.tokens).collect();
    let peak = app
        .trend_points
        .iter()
        .max_by_key(|p| (p.tokens, p.day))
        .expect("non-empty trend points");
    let total: u128 = app.trend_points.iter().map(|p| u128::from(p.tokens)).sum();
    let avg = (total / app.trend_points.len() as u128) as u64;
    let title = format::truncate_ellipsis(
        &format!(
            "Daily Trend · {scope} · peak {} {} · avg {}/d",
            peak.day,
            format::token_count(peak.tokens),
            format::token_count(avg)
        ),
        area.width.saturating_sub(4) as usize,
    );

    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border())
                .title(title),
        )
        .data(&values)
        .style(theme.bar_full());
    frame.render_widget(sparkline, area);
}

fn render_models_list(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let filtered_total = app.filtered_total_tokens();
    let items: Vec<ListItem<'_>> = app
        .model_rows
        .iter()
        .enumerate()
        .map(|(pos, row)| {
            let selected = app.selected_model_position() == Some(pos);
            let marker = if selected { "›" } else { " " };
            let style = if selected {
                theme.selected_row()
            } else {
                theme.primary_text()
            };
            let client_tag = format_client_tags(&row.clients);
            let total_str = format::token_count(row.tokens.total_tokens);
            let pct_str = format::percentage(row.tokens.total_tokens, filtered_total);
            let bar_width = if inner_width >= 52 {
                8
            } else if inner_width >= 34 {
                4
            } else {
                0
            };
            let bar_space = if bar_width > 0 { bar_width + 1 } else { 0 };
            let right_width =
                client_tag.len() + 1 + bar_space + total_str.len() + 1 + pct_str.len().max(4);
            let name_width = inner_width.saturating_sub(2 + right_width + 1).max(1);
            let name = format::truncate_ellipsis(&row.model, name_width);
            let gap = inner_width.saturating_sub(2 + name.chars().count() + right_width);

            let mut spans = vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(name, style),
                Span::raw(" ".repeat(gap)),
                Span::styled(client_tag, theme.muted_text()),
                Span::raw(" "),
            ];
            if bar_width > 0 {
                let bar = format::usage_bar(row.tokens.total_tokens, filtered_total, bar_width);
                spans.push(Span::styled(bar, theme.bar_full()));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(total_str, theme.number()));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format::pad_left(&pct_str, pct_str.len().max(4)),
                theme.percentage(),
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = if app.search.query.is_empty() {
        format!(
            "Models ({}) · {}",
            app.model_rows.len(),
            app.model_sort.label()
        )
    } else {
        format!(
            "Models /{} ({}) · {}",
            app.search.query,
            app.model_rows.len(),
            app.model_sort.label()
        )
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if app.focus == PaneFocus::Models {
                    theme.title()
                } else {
                    theme.border()
                })
                .title(title),
        )
        .highlight_style(theme.selected_row());
    frame.render_stateful_widget(list, area, &mut app.model_list_state);
}

fn render_model_detail(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let viewport_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2);
    let Some(row) = app.selected_model_row() else {
        app.set_detail_metrics(0, viewport_height);
        let msg = if app.snapshot.sessions.is_empty() {
            "No usage models found."
        } else {
            "No models match current filter. Press Esc to clear search or c/f to reset filters."
        };
        frame.render_widget(
            Paragraph::new(msg).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if app.focus == PaneFocus::Details {
                        theme.title()
                    } else {
                        theme.border()
                    })
                    .title("Model Detail"),
            ),
            area,
        );
        return;
    };

    let filtered_total = app.filtered_total_tokens();
    let title = format::truncate_ellipsis(&row.model, area.width.saturating_sub(4) as usize);
    let lines = model_detail_lines(
        row,
        filtered_total,
        app.breakdown,
        theme,
        inner_width as usize,
    );
    let total_lines = wrapped_line_count(&lines, inner_width as usize);
    app.set_detail_metrics(total_lines, viewport_height);
    let paragraph = Paragraph::new(lines)
        .scroll((app.detail_scroll.min(u16::MAX as usize) as u16, 0))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if app.focus == PaneFocus::Details {
                    theme.title()
                } else {
                    theme.border()
                })
                .title(title),
        );
    frame.render_widget(paragraph, area);
}

fn wrapped_line_count(lines: &[Line<'_>], width: usize) -> usize {
    if width == 0 {
        return lines.len();
    }
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

fn model_detail_lines(
    row: &ModelAggregateRow,
    filtered_total: u64,
    breakdown: bool,
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let clients = row
        .clients
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let pct = format::percentage(row.tokens.total_tokens, filtered_total);
    lines.push(Line::from(vec![
        Span::styled(row.model.clone(), theme.model_name()),
        Span::raw("  "),
        Span::styled(pct, theme.percentage()),
    ]));
    lines.push(Line::styled(
        format!(
            "Clients: {clients} · {} sessions · {} records",
            row.session_count, row.record_count
        ),
        theme.muted_text(),
    ));
    lines.push(Line::styled(
        format!(
            "Total {} ({})",
            row.tokens.total_tokens,
            format::token_count(row.tokens.total_tokens)
        ),
        theme.number(),
    ));
    let cache_denom = row.tokens.input_total.max(
        row.tokens
            .input_uncached
            .saturating_add(row.tokens.cache_read),
    );
    let cache_hit = format::percentage(row.tokens.cache_read, cache_denom);
    lines.push(Line::styled(
        format!("Cache hit rate {cache_hit}"),
        theme.primary_text(),
    ));
    lines.push(Line::raw(""));

    for (label, value) in [
        ("INPUT", row.tokens.input_total),
        ("Uncached", row.tokens.input_uncached),
        ("Cache read", row.tokens.cache_read),
        ("Cache write", row.tokens.cache_write),
        ("OUTPUT", row.tokens.output_total),
    ] {
        lines.push(Line::styled(
            format!("{label} {value}"),
            theme.primary_text(),
        ));
    }
    lines.push(Line::styled(
        format!(
            "REASONING {}{}",
            row.tokens.reasoning_known,
            if row.tokens.reasoning_has_unknown {
                "+ (some unknown)"
            } else {
                ""
            }
        ),
        theme.reasoning(),
    ));

    lines.push(Line::raw(""));
    lines.push(Line::styled("Effort Breakdown", theme.title()));
    for effort in &row.efforts {
        let label = effort
            .effort
            .as_ref()
            .map(|e| e.label())
            .unwrap_or_else(|| "none".into());
        let eff_pct = format::percentage(effort.tokens.total_tokens, row.tokens.total_tokens);
        let reason = format::reasoning(
            effort.tokens.reasoning_known,
            effort.tokens.reasoning_has_unknown,
        );
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
            Span::styled(format!("{:>6}", eff_pct), theme.percentage()),
            Span::raw("   reasoning "),
            Span::styled(format!("{:>8}", reason), theme.reasoning()),
        ]));
        if breakdown {
            lines.push(Line::styled(
                format::truncate_ellipsis(
                    &format!(
                        "    in {} · cache-r {} · cache-w {} · out {}",
                        effort.tokens.input_total,
                        effort.tokens.cache_read,
                        effort.tokens.cache_write,
                        effort.tokens.output_total
                    ),
                    width,
                ),
                theme.muted_text(),
            ));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled("Top Sessions", theme.title()));
    for (key, name, tokens) in &row.top_sessions {
        let tag = match key.client {
            Client::Codex => "C",
            Client::Gjc => "G",
            Client::OpenCode => "O",
        };
        let label = name.as_deref().unwrap_or(&key.id.0);
        let total = format::token_count(*tokens);
        let sess_pct = format::percentage(*tokens, row.tokens.total_tokens);
        let avail = width
            .saturating_sub(6 + total.len() + sess_pct.len() + 3)
            .max(1);
        let trunc = format::truncate_ellipsis(label, avail);
        let gap =
            width.saturating_sub(6 + trunc.chars().count() + total.len() + sess_pct.len() + 2);
        lines.push(Line::from(vec![
            Span::raw("  ["),
            Span::styled(
                tag,
                match key.client {
                    Client::Codex => theme.client_codex(),
                    Client::Gjc => theme.client_gjc(),
                    Client::OpenCode => theme.client_opencode(),
                },
            ),
            Span::raw("] "),
            Span::styled(trunc, theme.primary_text()),
            Span::raw(" ".repeat(gap)),
            Span::styled(total, theme.number()),
            Span::raw(" "),
            Span::styled(sess_pct, theme.percentage()),
        ]));
    }
    lines
}

fn format_client_tags(clients: &[Client]) -> String {
    let tags = clients
        .iter()
        .map(|c| match c {
            Client::Codex => "C",
            Client::Gjc => "G",
            Client::OpenCode => "O",
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("[{tags}]")
}
