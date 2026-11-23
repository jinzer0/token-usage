use crate::ui::{
    app::{App, PaneFocus},
    format,
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let pinned = app.pinned_session.is_some();
    let focused = app.focus == PaneFocus::Dates;
    let inner_width = area.width.saturating_sub(2) as usize;
    let focused_row_style = theme.selected_row_focused();

    let items = app
        .visible_dates
        .iter()
        .enumerate()
        .filter_map(|(position, index)| {
            let date = app.snapshot.dates.get(*index)?;
            let tokens = app.date_tokens(*index)?;
            let label = date
                .day
                .map(|day| day.to_string())
                .unwrap_or_else(|| "Unknown date".into());
            let selected = app.selected_date_position() == Some(position);
            let row_style = if selected && focused {
                focused_row_style
            } else if selected {
                theme.selected_row_unfocused()
            } else {
                theme.primary_text()
            };
            let mut lines = vec![Line::styled(
                format!("{} {}", if selected { "›" } else { " " }, label),
                row_style,
            )];

            let total_str = format::token_count(tokens.total_tokens);
            let delta_info = date.day.and_then(|_| {
                let previous = app
                    .visible_dates
                    .get(position + 1)
                    .and_then(|previous| {
                        app.snapshot
                            .dates
                            .get(*previous)
                            .and_then(|day| day.day)
                            .map(|_| *previous)
                    })
                    .and_then(|previous| app.date_tokens(previous))?;
                let delta = i128::from(tokens.total_tokens) - i128::from(previous.total_tokens);
                let magnitude = delta.unsigned_abs() as u64;
                let (text, style) = if delta > 0 {
                    (
                        format!("+{}", format::token_count(magnitude)),
                        theme.delta_positive(),
                    )
                } else if delta < 0 {
                    (
                        format!("-{}", format::token_count(magnitude)),
                        theme.delta_negative(),
                    )
                } else {
                    ("+0".into(), theme.muted_text())
                };
                Some((text, style))
            });

            if let Some((delta_str, delta_style)) = delta_info {
                let two_space_len =
                    2 + format::display_width(&total_str) + 2 + format::display_width(&delta_str);
                let one_space_len =
                    2 + format::display_width(&total_str) + 1 + format::display_width(&delta_str);
                let effective_delta_style = if selected && focused {
                    focused_row_style
                } else {
                    delta_style
                };
                if two_space_len <= inner_width {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {total_str}  "), row_style),
                        Span::styled(delta_str, effective_delta_style),
                    ]));
                } else if one_space_len <= inner_width {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {total_str} "), row_style),
                        Span::styled(delta_str, effective_delta_style),
                    ]));
                } else {
                    lines.push(Line::styled(format!("  {total_str}"), row_style));
                    lines.push(Line::styled(
                        format!("  {delta_str}"),
                        effective_delta_style,
                    ));
                }
            } else {
                lines.push(Line::styled(format!("  {total_str}"), row_style));
            }

            if pinned {
                let all_total = app
                    .client_date_tokens(*index)
                    .map(|t| t.total_tokens)
                    .unwrap_or(date.tokens.total_tokens);
                lines.push(Line::styled(
                    format!("  all {}", format::token_count(all_total)),
                    if selected && focused {
                        focused_row_style
                    } else {
                        theme.muted_text()
                    },
                ));
            }
            Some(ListItem::new(lines))
        })
        .collect::<Vec<_>>();

    let title = if pinned {
        "Dates · Pinned"
    } else {
        "Dates · All sessions"
    };
    let border_style = if focused {
        theme.border_focused()
    } else {
        theme.border()
    };
    let highlight_style = if focused {
        focused_row_style
    } else {
        Style::default()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title)
                .title_bottom(Line::from(" Δ prev usage day ").style(theme.muted_text())),
        )
        .highlight_style(highlight_style);
    frame.render_stateful_widget(list, area, &mut app.date_list_state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::ViewTab;
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    #[test]
    fn renders_compact_colored_deltas_and_focus_highlight() {
        let mut app = App::new(crate::ui::app::daily_test_snapshot([true; 3]));
        app.switch_tab(ViewTab::Dates);
        app.focus = PaneFocus::Dates;
        let mut terminal = Terminal::new(TestBackend::new(24, 12)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &mut app, &Theme::default()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("2026-03-11"));
        assert!(text.contains("300"));
        assert!(text.contains("+200"));
        assert!(text.contains("Δ prev usage day"));

        // Focused selected row uses Black on Cyan background
        let selected_cell = &buf[(1, 1)];
        assert_eq!(selected_cell.fg, Color::Black);
        assert_eq!(selected_cell.bg, Color::Cyan);

        // Positive delta on unfocused row uses Yellow
        app.focus = PaneFocus::Sessions;
        terminal
            .draw(|frame| render(frame, frame.area(), &mut app, &Theme::default()))
            .unwrap();
        let row2: String = (0..24)
            .map(|x| terminal.backend().buffer()[(x, 2)].symbol())
            .collect();
        let plus_x = row2.find("+200").expect("delta on same line");
        assert_eq!(
            terminal.backend().buffer()[(plus_x as u16, 2)].fg,
            Color::Yellow
        );
    }
}
