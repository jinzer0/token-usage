use crate::{
    domain::Client,
    ui::{app::App, format, theme::Theme},
};
use chrono::Local;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let session_count = app.snapshot.sessions.len();
    let total = app
        .snapshot
        .sessions
        .iter()
        .map(|s| s.tokens.total_tokens)
        .sum::<u64>();
    let codex = app
        .snapshot
        .sessions
        .iter()
        .filter(|s| s.key.client == Client::Codex)
        .count();
    let gjc = app
        .snapshot
        .sessions
        .iter()
        .filter(|s| s.key.client == Client::Gjc)
        .count();
    let refreshed = app
        .snapshot
        .generated_at
        .with_timezone(&Local)
        .format("%H:%M")
        .to_string();

    let mut spans = vec![
        Span::styled(" token-usage", theme.title()),
        Span::raw("     "),
        Span::styled(
            format!(
                "{session_count} sessions · {} tokens",
                format::token_count(total)
            ),
            theme.header(),
        ),
    ];
    if area.width >= 80 {
        spans.push(Span::raw("     "));
        spans.push(Span::styled(
            format!("codex {codex} · gjc {gjc}"),
            theme.muted_text(),
        ));
    }
    if area.width >= 60 {
        spans.push(Span::raw("     "));
        spans.push(Span::styled(
            format!("refreshed {refreshed}"),
            theme.muted_text(),
        ));
    }
    if !app.snapshot.diagnostics.is_empty() && area.width >= 100 {
        spans.push(Span::raw("     "));
        spans.push(Span::styled(
            format!("{} warnings", app.snapshot.diagnostics.len()),
            theme.warning(),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
