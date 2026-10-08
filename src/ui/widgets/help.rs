use crate::ui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let lines = vec![
        hint(theme, "j / ↓", "next session"),
        hint(theme, "k / ↑", "previous session"),
        hint(theme, "J / PgDn", "scroll detail down"),
        hint(theme, "K / PgUp", "scroll detail up"),
        hint(theme, "/", "search sessions"),
        hint(theme, "v", "toggle token breakdown"),
        hint(theme, "d", "select usage date"),
        hint(theme, "↑↓/Enter/Esc", "pick/apply/cancel"),
        hint(theme, "r", "reload"),
        hint(theme, "? / Esc / q", "close help"),
    ];
    let popup = centered(area, 48, lines.len().saturating_add(2) as u16);
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
