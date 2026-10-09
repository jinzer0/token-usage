use crate::ui::{
    app::{App, SearchMode, ViewTab},
    format,
    layout::LayoutMode,
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, _mode: LayoutMode) {
    let line = if app.search.mode == SearchMode::Editing {
        Line::from(vec![
            Span::styled(" Search: ", theme.key_hint()),
            Span::styled(app.search.query.clone(), theme.primary_text()),
            Span::styled("  Enter accept · Esc cancel", theme.muted_text()),
        ])
    } else if let Some(error) = &app.footer_error {
        Line::from(vec![
            Span::styled(" ERROR r retry · q quit · ", theme.error()),
            Span::styled(error.clone(), theme.error()),
        ])
    } else if let Some(notice) = &app.footer_notice {
        let hints = "  r reload · ? help · q quit";
        Line::from(vec![
            Span::styled(
                format::truncate_ellipsis(
                    notice,
                    area.width.saturating_sub(hints.len() as u16) as usize,
                ),
                theme.muted_text(),
            ),
            Span::styled(hints, theme.key_hint()),
        ])
    } else {
        let hint = if app.tab == ViewTab::Dates {
            " 1/2 tabs · Tab pane · jk move · [/] dates · s sort · d/p pin · / search · v breakdown · r reload · ? · q"
        } else if area.width >= 100 {
            " 1/2 tabs · jk session · d dates · s sort · / search · J/K scroll · v breakdown · r reload · ? help · q quit"
        } else {
            " jk · d dates · s sort · ? help · q"
        };
        Line::styled(hint, theme.key_hint())
    };
    frame.render_widget(Paragraph::new(line), area);
}
