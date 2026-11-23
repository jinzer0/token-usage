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
        let mut spans = vec![
            Span::styled(" Search: ", theme.hint_key()),
            Span::styled(app.search.query.clone(), theme.primary_text()),
            Span::raw("  "),
        ];
        push_hints(&mut spans, &[("Enter", "accept"), ("Esc", "cancel")], theme);
        Line::from(spans)
    } else if let Some(error) = &app.footer_error {
        let mut spans = vec![Span::styled(" ERROR ", theme.error())];
        push_hints(&mut spans, &[("r", "retry"), ("q", "quit")], theme);
        spans.push(Span::styled(" · ", theme.muted_text()));
        spans.push(Span::styled(error.clone(), theme.error()));
        Line::from(spans)
    } else if let Some(notice) = &app.footer_notice {
        let hints_text = "  r reload · ? help · q quit";
        let mut spans = vec![
            Span::raw(" "),
            Span::styled(
                format::truncate_ellipsis(
                    notice,
                    area.width.saturating_sub(hints_text.len() as u16 + 1) as usize,
                ),
                theme.muted_text(),
            ),
            Span::raw("  "),
        ];
        push_hints(
            &mut spans,
            &[("r", "reload"), ("?", "help"), ("q", "quit")],
            theme,
        );
        Line::from(spans)
    } else {
        let has_filter = app.search.mode == SearchMode::Confirmed && !app.search.query.is_empty();
        let mut spans = vec![Span::raw(" ")];
        if has_filter {
            spans.push(Span::styled(
                format!("Filter: /{} (Esc clear)", app.search.query),
                theme.hint_key(),
            ));
            spans.push(Span::styled(" · ", theme.muted_text()));
        }
        let pairs: &[(&str, &str)] =
            if app.tab == ViewTab::Models && (!has_filter || area.width >= 136) {
                &[
                    ("1-3", "tabs"),
                    ("Tab", "pane"),
                    ("jk", "move"),
                    ("s", "sort"),
                    ("c", "client"),
                    ("f", "period"),
                    ("/", "search"),
                    ("v", "breakdown"),
                    ("r", "reload"),
                    ("?", "help"),
                    ("q", "quit"),
                ]
            } else if app.tab == ViewTab::Dates && (!has_filter || area.width >= 136) {
                &[
                    ("1-3", "tabs"),
                    ("Tab", "pane"),
                    ("jk", "move"),
                    ("[/]", "dates"),
                    ("s", "sort"),
                    ("c/f", "filter"),
                    ("d/p", "pin"),
                    ("/", "search"),
                    ("v", "detail"),
                    ("r", ""),
                    ("?", ""),
                    ("q", ""),
                ]
            } else if area.width >= 100 && (!has_filter || area.width >= 136) {
                &[
                    ("1-3", "tabs"),
                    ("jk", "session"),
                    ("d", "dates"),
                    ("s", "sort"),
                    ("c/f", "filter"),
                    ("/", "search"),
                    ("v", "breakdown"),
                    ("r", "reload"),
                    ("?", "help"),
                    ("q", "quit"),
                ]
            } else if area.width >= 100 {
                &[
                    ("1-3", "tabs"),
                    ("jk", "move"),
                    ("s", "sort"),
                    ("c/f", "filter"),
                    ("v", "breakdown"),
                    ("r", "reload"),
                    ("?", "help"),
                    ("q", "quit"),
                ]
            } else {
                &[
                    ("jk", ""),
                    ("d", "dates"),
                    ("c/f", "filter"),
                    ("?", "help"),
                    ("q", ""),
                ]
            };
        push_hints(&mut spans, pairs, theme);
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn push_hints(
    spans: &mut Vec<Span<'static>>,
    pairs: &[(&'static str, &'static str)],
    theme: &Theme,
) {
    for (idx, (key, label)) in pairs.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" · ", theme.muted_text()));
        }
        spans.push(Span::styled(*key, theme.hint_key()));
        if !label.is_empty() {
            spans.push(Span::styled(format!(" {label}"), theme.hint_desc()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    #[test]
    fn confirmed_search_shows_filter_badge_and_split_hint_styles() {
        let mut app = App::new(crate::ui::app::daily_test_snapshot([true; 3]));
        app.start_search();
        app.push_search_char('g');
        app.push_search_char('j');
        app.confirm_search();
        let mut terminal = Terminal::new(TestBackend::new(110, 1)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &app,
                    &Theme::default(),
                    LayoutMode::Wide,
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let row: String = (0..110).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(row.contains("Filter: /gj (Esc clear)"));
        let byte_pos = row.find("q quit").unwrap();
        let q_col = row[..byte_pos].chars().count() as u16;
        assert_eq!(buf[(q_col, 0)].fg, Color::Yellow);
        assert_eq!(buf[(q_col + 2, 0)].fg, Color::Gray);
    }
}
