mod footer;
mod header;
mod help;
mod session_detail;
mod session_list;

use crate::ui::{app::App, layout, theme::Theme};
use ratatui::{Frame, widgets::Paragraph};

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    let theme = Theme::default();
    let area = frame.area();
    let layout = layout::calculate(area);

    if layout.mode == layout::LayoutMode::TooSmall {
        frame.render_widget(
            Paragraph::new("Terminal too small\nMinimum recommended: 60x15"),
            area,
        );
        return;
    }

    header::render(frame, layout.header, app, &theme);
    if let Some(sessions) = layout.sessions {
        session_list::render(frame, sessions, app, &theme);
    }
    session_detail::render(frame, layout.detail, app, &theme, layout.mode);
    footer::render(frame, layout.footer, app, &theme, layout.mode);
    if app.help_visible {
        help::render(frame, area, &theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        aggregate::{AggregateSnapshot, EffortStats, ModelStats, SessionStats},
        domain::{Client, ReasoningEffort, SessionId, SessionKey, TokenStats},
    };
    use chrono::Utc;
    use ratatui::{Terminal, backend::TestBackend};

    fn sample_snapshot() -> AggregateSnapshot {
        AggregateSnapshot {
            generated_at: Utc::now(),
            sessions: vec![SessionStats {
                key: SessionKey {
                    client: Client::Gjc,
                    id: SessionId("session-1".into()),
                },
                parent: None,
                name: Some("long-review-session".into()),
                started_at: None,
                tokens: TokenStats {
                    total_tokens: 1_000,
                    input_total: 700,
                    output_total: 300,
                    reasoning_known: 120,
                    ..Default::default()
                },
                models: vec![ModelStats {
                    model: "gpt-test".into(),
                    tokens: TokenStats {
                        total_tokens: 1_000,
                        input_total: 700,
                        output_total: 300,
                        reasoning_known: 120,
                        ..Default::default()
                    },
                    efforts: vec![EffortStats {
                        effort: Some(ReasoningEffort::Custom("xhigh".into())),
                        tokens: TokenStats {
                            total_tokens: 1_000,
                            input_total: 700,
                            output_total: 300,
                            reasoning_known: 120,
                            ..Default::default()
                        },
                        record_count: 1,
                    }],
                    record_count: 1,
                }],
                record_count: 1,
            }],
            diagnostics: vec![],
            source_counts: Default::default(),
        }
    }

    #[test]
    fn renders_wide_tui_core_text() {
        let backend = TestBackend::new(140, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(sample_snapshot());
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let content = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(content.contains("token-usage"));
        assert!(content.contains("long-review-session"));
        assert!(content.contains("gpt-test"));
        assert!(content.contains("xhigh"));
    }

    #[test]
    fn renders_empty_snapshot() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(AggregateSnapshot {
            generated_at: Utc::now(),
            sessions: vec![],
            diagnostics: vec![],
            source_counts: Default::default(),
        });
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let content = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(content.contains("No usage sessions found"));
    }
}
