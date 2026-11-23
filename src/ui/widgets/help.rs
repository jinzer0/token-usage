use crate::ui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let grouped = area.height >= 22;
    let lines = if grouped {
        vec![
            group_header(theme, "Navigation"),
            hint(theme, "1 / 2 / 3", "Sessions / Dates tabs / Models"),
            hint(theme, "Tab / ←→", "focus pane; j/k or ↑↓ move"),
            hint(theme, "g / G", "first / last item; PgUp/PgDn page"),
            hint(theme, "[ / ]", "date; Δ versus previous usage day"),
            hint(
                theme,
                "J/K / Pg",
                "scroll detail; exact totals in breakdown",
            ),
            Line::raw(""),
            group_header(theme, "Actions & Filters"),
            hint(theme, "c / f", "cycle client / period filter"),
            hint(theme, "F / n", "custom N days filter"),
            hint(theme, "d / p", "open pinned dates / toggle session pin"),
            hint(theme, "s", "Last used / Tokens (selected day in Dates)"),
            hint(theme, "/", "search sessions, not global date totals"),
            hint(theme, "v", "toggle token breakdown"),
            Line::raw(""),
            group_header(theme, "General"),
            hint(theme, "r", "reload (mouse click/wheel supported)"),
            hint(theme, "q / Ctrl+C", "quit (Esc clears active search first)"),
            hint(theme, "? / Esc / q", "close help"),
        ]
    } else {
        vec![
            hint(theme, "1 / 2 / 3", "Sessions / Dates tabs / Models"),
            hint(theme, "Tab / ←→", "focus pane; j/k ↑↓ g/G move"),
            hint(theme, "[ / ]", "date; Δ versus previous usage day"),
            hint(
                theme,
                "c / f / s",
                "client / period / sort (selected day in Dates)",
            ),
            hint(theme, "F / n", "custom N days filter"),
            hint(theme, "d / p / /", "pinned dates / pin / search"),
            hint(theme, "J/K / v", "scroll detail / toggle breakdown"),
            hint(theme, "r / Ctrl+C", "reload / quit (Esc clears search)"),
            hint(theme, "? / Esc / q", "close help"),
        ]
    };

    let popup = centered(area, 72, lines.len().saturating_add(2) as u16);
    let border_style = theme.border_focused();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Line::styled(" Help · Keybindings ", border_style)),
        ),
        popup,
    );
}

fn group_header(theme: &Theme, title: &'static str) -> Line<'static> {
    Line::styled(format!(" {title}"), theme.border_focused())
}

fn hint(theme: &Theme, key: &'static str, label: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:<12}"), theme.key_hint()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn help_shows_grouped_headers_when_tall_and_fits_minimum_height() {
        let mut tall = Terminal::new(TestBackend::new(80, 24)).unwrap();
        tall.draw(|frame| render(frame, frame.area(), &Theme::default()))
            .unwrap();
        let tall_text: String = tall
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(tall_text.contains("Navigation"));
        assert!(tall_text.contains("Actions & Filters"));
        assert!(tall_text.contains("close help"));

        let mut short = Terminal::new(TestBackend::new(110, 12)).unwrap();
        short
            .draw(|frame| render(frame, frame.area(), &Theme::default()))
            .unwrap();
        let short_text: String = short
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(short_text.contains("close help"));
    }
}
