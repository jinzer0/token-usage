use crate::ui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let lines = vec![
        hint(theme, "1 / 2", "Sessions / Dates tabs"),
        hint(theme, "Tab / ←→", "focus pane; j/k or ↑↓ move"),
        hint(theme, "[ / ]", "date; Δ versus previous usage day"),
        hint(theme, "d / p", "open pinned dates / toggle session pin"),
        hint(theme, "s", "Last used / Tokens (selected day in Dates)"),
        hint(theme, "/", "search sessions, not global date totals"),
        hint(
            theme,
            "J/K / Pg",
            "scroll detail; exact totals in breakdown",
        ),
        hint(theme, "v", "toggle token breakdown"),
        hint(theme, "r", "reload"),
        hint(theme, "? / Esc / q", "close help"),
    ];
    let popup = centered(area, 70, lines.len().saturating_add(2) as u16);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border())
                .title("Help"),
        ),
        popup,
    );
}

fn hint(theme: &Theme, key: &'static str, label: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:<10}"), theme.key_hint()),
        Span::styled(label, theme.primary_text()),
    ])
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}
