use crate::ui::{
    app::{App, PaneFocus},
    format,
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, List, ListItem},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let pinned = app.pinned_session.is_some();
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
            let style = if selected {
                theme.selected_row()
            } else {
                theme.primary_text()
            };
            let mut lines = vec![Line::styled(
                format!("{} {}", if selected { "›" } else { " " }, label),
                style,
            )];
            lines.push(Line::styled(
                format!("  {}", format::token_count(tokens.total_tokens)),
                style,
            ));
            if date.day.is_some() {
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
                    .and_then(|previous| app.date_tokens(previous));
                if let Some(previous) = previous {
                    let delta = i128::from(tokens.total_tokens) - i128::from(previous.total_tokens);
                    let magnitude = delta.unsigned_abs() as u64;
                    lines.push(Line::styled("  Δ prev usage day", theme.muted_text()));
                    lines.push(Line::styled(
                        format!(
                            "  {}{}",
                            if delta < 0 { "-" } else { "+" },
                            format::token_count(magnitude)
                        ),
                        theme.muted_text(),
                    ));
                }
            }
            if pinned {
                lines.push(Line::styled(
                    format!("  all {}", format::token_count(date.tokens.total_tokens)),
                    theme.muted_text(),
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
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if app.focus == PaneFocus::Dates {
                    theme.title()
                } else {
                    theme.border()
                })
                .title(title),
        )
        .highlight_style(theme.selected_row());
    frame.render_stateful_widget(list, area, &mut app.date_list_state);
}
