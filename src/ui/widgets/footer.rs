use crate::ui::{
    app::{App, SearchMode},
    layout::LayoutMode,
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, mode: LayoutMode) {
    if let Some(error) = &app.footer_error {
        let action = if app.date_picker.is_some() {
            "Esc cancel · "
        } else {
            "r retry · q quit · "
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ERROR ", theme.error()),
                Span::styled(action, theme.key_hint()),
                Span::styled(error.clone(), theme.error()),
            ])),
            area,
        );
        return;
    }

    if app.date_picker.is_some() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                " ↑↓ date · Enter apply · Esc cancel",
                theme.key_hint(),
            )),
            area,
        );
        return;
    }

    if app.search.mode == SearchMode::Editing {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Search: ", theme.key_hint()),
                Span::styled(app.search.query.clone(), theme.primary_text()),
                Span::styled("  Enter accept · Esc cancel", theme.muted_text()),
            ])),
            area,
        );
        return;
    }

    if let Some(notice) = &app.footer_notice {
        let hints = if app.date_picker_available {
            " d date r reload q quit"
        } else {
            " r reload q quit"
        };
        let width = area.width.saturating_sub(hints.len() as u16) as usize;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    crate::ui::format::truncate_ellipsis(notice, width),
                    theme.muted_text(),
                ),
                Span::styled(hints, theme.key_hint()),
            ])),
            area,
        );
        return;
    }

    let pos = app.selected_filtered_position().map(|p| p + 1).unwrap_or(0);
    let total = app.filtered_sessions.len();
    let mut spans = Vec::new();
    if mode == LayoutMode::Narrow {
        spans.push(Span::styled(
            format!(" {pos}/{total}  "),
            theme.muted_text(),
        ));
        push_hint(&mut spans, theme, "j/k", "session");
    } else {
        spans.push(Span::raw(" "));
        push_hint(&mut spans, theme, "↑↓/jk", "navigate");
    }
    push_hint(&mut spans, theme, "/", "search");
    if app.date_picker_available {
        push_hint(&mut spans, theme, "d", "date");
    }
    push_hint(&mut spans, theme, "J/K", "scroll");
    push_hint(
        &mut spans,
        theme,
        "v",
        if app.breakdown {
            "summary"
        } else {
            "breakdown"
        },
    );
    push_hint(&mut spans, theme, "r", "refresh");
    push_hint(&mut spans, theme, "?", "help");
    push_hint(&mut spans, theme, "q", "quit");
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn push_hint(
    spans: &mut Vec<Span<'static>>,
    theme: &Theme,
    key: &'static str,
    label: &'static str,
) {
    spans.push(Span::styled(key, theme.key_hint()));
    spans.push(Span::styled(format!(" {label}   "), theme.muted_text()));
}
