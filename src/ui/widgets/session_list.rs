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
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let inner_width = area.width.saturating_sub(2) as usize;
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
            let client_style = match session.key.client {
                Client::Codex => theme.client_codex(),
                Client::Gjc => theme.client_gjc(),
                Client::OpenCode => theme.client_opencode(),
            };
            let total = format::token_count(app.selected_tokens(*idx)?.total_tokens);
            let prefix_width = 6usize;
            let total_width = total.chars().count().max(4);
            let name_width = inner_width
                .saturating_sub(prefix_width + total_width + 1)
                .max(1);
            let name = format::truncate_ellipsis(
                session.name.as_deref().unwrap_or(&session.key.id.0),
                name_width,
            );
            let gap = inner_width.saturating_sub(prefix_width + name.chars().count() + total_width);
            let marker = if selected { "›" } else { " " };
            let style = if selected {
                theme.selected_row()
            } else {
                theme.primary_text()
            };
            Some(ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::raw(" ["),
                Span::styled(client, client_style),
                Span::raw("] "),
                Span::styled(name, style),
                Span::raw(" ".repeat(gap)),
                Span::styled(total, theme.number()),
            ])))
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
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if app.focus == PaneFocus::Sessions {
                    theme.title()
                } else {
                    theme.border()
                })
                .title(title),
        )
        .highlight_style(theme.selected_row());
    frame.render_stateful_widget(list, area, &mut app.session_list_state);
}
