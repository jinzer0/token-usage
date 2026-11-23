use crate::{
    domain::Client,
    ui::{
        app::{App, PaneFocus, ViewTab},
        format,
        theme::Theme,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let show_age = inner_width >= 34;
    let focused = app.focus == PaneFocus::Sessions;
    let focused_row_style = theme.selected_row_focused();
    let now = app.snapshot.generated_at;

    let items = app
        .filtered_sessions
        .iter()
        .enumerate()
        .filter_map(|(pos, idx)| {
            let session = &app.snapshot.sessions[*idx];
            let selected = app.selected_filtered_position() == Some(pos);
            let client = match session.key.client {
                Client::Codex => "C",
                Client::Gjc => "G",
                Client::OpenCode => "O",
            };
            let client_style = if selected && focused {
                focused_row_style
            } else {
                match session.key.client {
                    Client::Codex => theme.client_codex(),
                    Client::Gjc => theme.client_gjc(),
                    Client::OpenCode => theme.client_opencode(),
                }
            };
            let total = format::token_count(app.selected_tokens(*idx)?.total_tokens);
            let total_width = format::display_width(&total).max(5);
            let total_padded = format::pad_left(&total, total_width);

            let age = if show_age {
                session
                    .last_used_at
                    .map(|ts| format::pad_left(&format::relative_time(ts, now), 5))
            } else {
                None
            };
            let right_width = match &age {
                Some(age_col) => format::display_width(age_col) + 1 + total_width,
                None => total_width,
            };

            let prefix_width = 6usize;
            let name_width = inner_width
                .saturating_sub(prefix_width + right_width + 1)
                .max(1);
            let name = format::truncate_ellipsis(
                session.name.as_deref().unwrap_or(&session.key.id.0),
                name_width,
            );
            let gap = inner_width
                .saturating_sub(prefix_width + format::display_width(&name) + right_width);
            let marker = if selected { "›" } else { " " };
            let text_style = if selected && focused {
                focused_row_style
            } else if selected {
                theme.selected_row_unfocused()
            } else {
                theme.primary_text()
            };
            let mut spans = vec![
                Span::styled(marker, text_style),
                Span::raw(" ["),
                Span::styled(client, client_style),
                Span::raw("] "),
                Span::styled(name, text_style),
                Span::raw(" ".repeat(gap)),
            ];
            if let Some(age_col) = age {
                spans.push(Span::styled(
                    format!("{age_col} "),
                    if selected && focused {
                        focused_row_style
                    } else {
                        theme.muted_text()
                    },
                ));
            }
            spans.push(Span::styled(
                total_padded,
                if selected && focused {
                    focused_row_style
                } else {
                    theme.number()
                },
            ));
            Some(ListItem::new(Line::from(spans)))
        })
        .collect::<Vec<_>>();

    let title = if app.search.query.is_empty() {
        format!("Sessions · {}", app.sort.label())
    } else {
        format!("Sessions /{} · {}", app.search.query, app.sort.label())
    };
    let title = if app.tab == ViewTab::Dates && app.pinned_session.is_some() {
        format!("{title} · pinned")
    } else {
        title
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
                .title(title),
        )
        .highlight_style(highlight_style);
    frame.render_stateful_widget(list, area, &mut app.session_list_state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn session_list_shows_age_only_when_wide_enough() {
        let mut app = App::new(crate::ui::app::daily_test_snapshot([true; 3]));
        let mut wide = Terminal::new(TestBackend::new(40, 8)).unwrap();
        wide.draw(|frame| render(frame, frame.area(), &mut app, &Theme::default()))
            .unwrap();
        let wide_text: String = wide
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(wide_text.contains("03-11") || wide_text.contains("03-10"));

        let mut narrow = Terminal::new(TestBackend::new(30, 8)).unwrap();
        narrow
            .draw(|frame| render(frame, frame.area(), &mut app, &Theme::default()))
            .unwrap();
        let narrow_text: String = narrow
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!narrow_text.contains("03-11"));
    }
}
