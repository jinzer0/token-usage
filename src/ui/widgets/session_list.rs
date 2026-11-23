use crate::{
    domain::Client,
    ui::{
        app::{App, PaneFocus, ViewTab},
        format,
        theme::Theme,
    },
};
use chrono::{DateTime, Local, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let show_age = inner_width >= 34;
    let focused = app.focus == PaneFocus::Sessions;
    let focused_row_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
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
            let total_width = total.chars().count().max(5);
            let total_padded = format::pad_left(&total, total_width);

            let age = if show_age {
                relative_age(session.last_used_at, now).map(|s| format!("{s:>5}"))
            } else {
                None
            };
            let right_width = match &age {
                Some(age_col) => age_col.chars().count() + 1 + total_width,
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
            let gap = inner_width.saturating_sub(prefix_width + name.chars().count() + right_width);
            let marker = if selected { "›" } else { " " };
            let text_style = if selected && focused {
                focused_row_style
            } else if selected {
                theme.selected_row()
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
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
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

fn relative_age(last_used_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<String> {
    let ts = last_used_at?;
    let elapsed = now.signed_duration_since(ts);
    let secs = elapsed.num_seconds();
    if secs < 60 {
        Some("now".into())
    } else if secs < 3600 {
        Some(format!("{}m", secs / 60))
    } else if secs < 86_400 {
        Some(format!("{}h", secs / 3600))
    } else if secs < 7 * 86_400 {
        Some(format!("{}d", secs / 86_400))
    } else {
        Some(ts.with_timezone(&Local).format("%m-%d").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn relative_age_formats_expected_buckets() {
        let now = Utc::now();
        assert_eq!(relative_age(None, now), None);
        assert_eq!(
            relative_age(Some(now - Duration::seconds(20)), now).as_deref(),
            Some("now")
        );
        assert_eq!(
            relative_age(Some(now - Duration::minutes(5)), now).as_deref(),
            Some("5m")
        );
        assert_eq!(
            relative_age(Some(now - Duration::hours(3)), now).as_deref(),
            Some("3h")
        );
        assert_eq!(
            relative_age(Some(now - Duration::days(2)), now).as_deref(),
            Some("2d")
        );
    }

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
