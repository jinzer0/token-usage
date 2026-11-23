use crate::{
    domain::Client,
    ui::{
        app::{App, ViewTab},
        format,
        theme::Theme,
    },
};
use chrono::Local;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let active_tab_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let inactive_tab_style = Style::default().fg(Color::Gray);

    let mut tabs = vec![
        Span::styled(" token-usage ", theme.title()),
        Span::raw(" "),
        Span::styled(
            " 1 Sessions ",
            if app.tab == ViewTab::Sessions {
                active_tab_style
            } else {
                inactive_tab_style
            },
        ),
        Span::raw(" "),
        Span::styled(
            " 2 Dates ",
            if app.tab == ViewTab::Dates {
                active_tab_style
            } else {
                inactive_tab_style
            },
        ),
    ];
    if area.width >= 110 {
        let count = |client| {
            app.snapshot
                .sessions
                .iter()
                .filter(|session| session.key.client == client)
                .count()
        };
        tabs.push(Span::styled(
            format!(
                "   codex {} · gjc {} · opencode {}",
                count(Client::Codex),
                count(Client::Gjc),
                count(Client::OpenCode)
            ),
            theme.muted_text(),
        ));
    }
    if area.width >= 80 {
        tabs.push(Span::styled(
            format!(
                "   refreshed {}",
                app.snapshot
                    .generated_at
                    .with_timezone(&Local)
                    .format("%H:%M")
            ),
            theme.muted_text(),
        ));
    }
    let mut summary = vec![Span::styled(
        format!(
            " All {} sessions · {} tokens",
            app.snapshot.sessions.len(),
            format::token_count(app.snapshot.totals.total_tokens)
        ),
        theme.header(),
    )];
    if app.tab == ViewTab::Dates {
        if let Some(date) = app.selected_date_stats() {
            let scope = app
                .selected_date
                .map(|scope| scope.label())
                .unwrap_or_default();
            summary.push(Span::styled(
                format!(
                    "   {scope} · all {}",
                    format::token_count(date.tokens.total_tokens)
                ),
                theme.muted_text(),
            ));
        }
    } else if area.width >= 110 {
        summary.push(Span::styled(
            format!(
                "   today {} · 7d {} · 30d {}",
                format::token_count(app.snapshot.periods.today.total_tokens),
                format::token_count(app.snapshot.periods.seven_days.total_tokens),
                format::token_count(app.snapshot.periods.thirty_days.total_tokens)
            ),
            theme.muted_text(),
        ));
    }
    if app.snapshot.source_counts.errors > 0 {
        summary.push(Span::styled(
            format!("   {} errors", app.snapshot.source_counts.errors),
            theme.error(),
        ));
    } else if !app.snapshot.diagnostics.is_empty() && area.width >= 130 {
        summary.push(Span::styled(
            format!("   {} warnings", app.snapshot.diagnostics.len()),
            theme.warning(),
        ));
    }
    frame.render_widget(
        Paragraph::new(vec![Line::from(tabs), Line::from(summary)]),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn active_tab_has_cyan_pill_style() {
        let app = App::new(crate::ui::app::daily_test_snapshot([true; 3]));
        let mut terminal = Terminal::new(TestBackend::new(110, 2)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &app, &Theme::default()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let row0: String = (0..110).map(|x| buf[(x, 0)].symbol()).collect();
        let s_pos = row0.find(" 1 Sessions ").unwrap();
        let d_pos = row0.find(" 2 Dates ").unwrap();
        assert_eq!(buf[(s_pos as u16 + 1, 0)].bg, Color::Cyan);
        assert_eq!(buf[(s_pos as u16 + 1, 0)].fg, Color::Black);
        assert_eq!(buf[(d_pos as u16 + 1, 0)].fg, Color::Gray);
    }
}
