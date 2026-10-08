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
    app.update_picker_availability(
        layout.mode != layout::LayoutMode::TooSmall
            && session_detail::picker_available(layout.detail),
    );

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
        let mut snapshot = AggregateSnapshot {
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
                daily: vec![],
            }],
            timeline: vec![],
            periods: Default::default(),
            diagnostics: vec![],
            source_counts: Default::default(),
        };
        let session = &mut snapshot.sessions[0];
        session.daily = vec![crate::aggregate::SessionDayStats {
            day: None,
            tokens: session.tokens.clone(),
            models: session.models.clone(),
            record_count: session.record_count,
        }];
        snapshot
    }

    fn opencode_snapshot() -> AggregateSnapshot {
        let mut snapshot = sample_snapshot();
        let session = &mut snapshot.sessions[0];
        session.key.client = Client::OpenCode;
        session.key.id = SessionId("root-session".into());
        session.name = Some("opencode-root".into());
        session.record_count = 5;
        let model = &mut session.models[0];
        model.model = "claude-opencode".into();
        model.tokens.total_tokens = 600;
        model.tokens.input_total = 420;
        model.tokens.output_total = 180;
        model.tokens.reasoning_known = 72;
        model.efforts[0].effort = None;
        model.efforts[0].tokens = model.tokens.clone();
        model.efforts[0].record_count = 3;
        model.record_count = 3;
        let mut second = model.clone();
        second.model = "gpt-opencode".into();
        second.tokens.total_tokens = 400;
        second.tokens.input_total = 280;
        second.tokens.output_total = 120;
        second.tokens.reasoning_known = 48;
        second.efforts[0].tokens = second.tokens.clone();
        second.efforts[0].record_count = 2;
        second.record_count = 2;
        session.models.push(second);
        session.daily = vec![crate::aggregate::SessionDayStats {
            day: None,
            tokens: session.tokens.clone(),
            models: session.models.clone(),
            record_count: session.record_count,
        }];
        snapshot
    }

    #[test]
    fn renders_opencode_root_label_detail_models_and_header() {
        let mut snapshot = opencode_snapshot();
        for client in [Client::Codex, Client::Gjc] {
            let mut session = sample_snapshot().sessions.remove(0);
            session.key.client = client;
            snapshot.sessions.push(session);
        }
        let backend = TestBackend::new(180, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(snapshot);
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let content = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(content.contains("[O] opencode-root"));
        assert!(content.contains("[C]"));
        assert!(content.contains("[G]"));
        assert!(content.contains("OPENCODE"));
        assert!(content.contains("claude-opencode"));
        assert!(content.contains("gpt-opencode"));
        assert!(content.contains("60%"));
        assert!(content.contains("40%"));
        assert!(content.contains("codex 1 · gjc 1 · opencode 1"));
        assert!(content.contains("3 sessions"));
        for client in ["CODEX", "GJC"] {
            app.move_down();
            terminal.draw(|frame| render(frame, &mut app)).unwrap();
            let content = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(content.contains(client));
            assert!(content.contains("gpt-test"));
            assert!(content.contains("xhigh"));
        }
        let label = terminal
            .backend()
            .buffer()
            .content()
            .windows(3)
            .find(|cells| {
                cells[0].symbol() == "[" && cells[1].symbol() == "O" && cells[2].symbol() == "]"
            })
            .unwrap();
        assert_eq!(label[1].fg, ratatui::style::Color::Green);
    }

    #[test]
    fn renders_opencode_at_narrow_layout_boundaries() {
        for width in [1, 39, 40, 59, 60, 79, 80, 109, 110] {
            for breakdown in [false, true] {
                let backend = TestBackend::new(width, 24);
                let mut terminal = Terminal::new(backend).unwrap();
                let mut app = App::new(opencode_snapshot());
                app.breakdown = breakdown;
                terminal.draw(|frame| render(frame, &mut app)).unwrap();
                let content = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                if width >= 40 {
                    assert!(content.contains("OPENCODE"), "width {width}");
                    assert!(content.contains("claude-opencode"), "width {width}");
                    assert!(content.contains("gpt-opencode"), "width {width}");
                    if width >= 80 {
                        assert!(content.contains("[O] opencode-root"), "width {width}");
                    }
                } else if width == 39 {
                    assert!(content.contains("Terminal too small"));
                }
            }
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
            timeline: vec![],
            periods: Default::default(),
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
        assert!(content.contains("Local stores: Codex, GJC, OpenCode."));
    }

    fn daily_screen(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn daily_whole_app_narrow_wide_modal_footer_and_help() {
        use crate::ui::app::{DetailScope, daily_test_snapshot};
        for (width, height) in [(140, 40), (60, 15), (40, 12)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(daily_test_snapshot([true; 3]));
            terminal.draw(|f| render(f, &mut app)).unwrap();
            assert!(app.date_picker_available);
            app.open_date_picker();
            terminal.draw(|f| render(f, &mut app)).unwrap();
            let screen = daily_screen(&terminal);
            assert!(
                screen.contains("Usage date") && screen.contains("Esc cancel"),
                "{width}x{height}"
            );
            app.footer_error = Some("controlled reload failure".into());
            terminal.draw(|f| render(f, &mut app)).unwrap();
            let screen = daily_screen(&terminal);
            assert!(screen.contains("ERROR") && screen.contains("Esc cancel"));
            assert!(!screen.contains("retry") && !screen.contains("q quit"));
            app.footer_error = None;
            app.move_date_cursor(1);
            app.apply_date_picker();
            assert!(matches!(app.detail_scope, DetailScope::Day(_)));
            app.breakdown = true;
            terminal.draw(|f| render(f, &mut app)).unwrap();
            assert!(daily_screen(&terminal).contains("day-new"));
            assert!(!daily_screen(&terminal).contains("day-unknown"));
            app.help_visible = true;
            terminal.draw(|f| render(f, &mut app)).unwrap();
            let screen = daily_screen(&terminal);
            assert!(
                screen.contains("select usage date")
                    && screen.contains("pick/apply/cancel")
                    && screen.contains("close help"),
                "{width}x{height}"
            );
        }
    }

    #[test]
    fn daily_long_list_zero_token_dates_and_resize_cancel_only_draft() {
        use crate::{
            aggregate::aggregate, domain::UsageRecord, scan::ParseResult, ui::app::DetailScope,
        };
        let start = "2026-01-01T12:00:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();
        let mut result = ParseResult::empty(Client::Gjc, "long-date-fixture".into());
        for index in 0..50 {
            result.records.push(UsageRecord {
                session_key: SessionKey {
                    client: Client::Gjc,
                    id: SessionId("long-dates".into()),
                },
                parent_session_key: None,
                message_id: None,
                source_path: "long-date-fixture".into(),
                source_line: None,
                started_at: Some(start + chrono::Duration::days(index)),
                session_name: None,
                model: "zero-token-model".into(),
                reasoning_effort: None,
                tokens: TokenStats::default(),
            });
        }
        let mut app = App::new(aggregate(vec![result]));
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();
        app.open_date_picker();
        assert_eq!(app.date_picker.as_ref().unwrap().options.len(), 51);
        app.move_date_cursor(isize::MAX);
        let selected = app.date_picker.as_ref().unwrap().options[50];
        terminal.draw(|f| render(f, &mut app)).unwrap();
        assert!(daily_screen(&terminal).contains(&selected.label()));
        app.apply_date_picker();
        assert_eq!(app.detail_scope, selected);
        assert!(matches!(selected, DetailScope::Day(_)));
        app.detail_scroll = 3;
        app.open_date_picker();
        app.move_date_cursor(isize::MIN);
        assert_eq!(app.detail_scope, selected);
        terminal.backend_mut().resize(20, 6);
        terminal
            .resize(ratatui::layout::Rect::new(0, 0, 20, 6))
            .unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();
        assert!(app.date_picker.is_none() && !app.date_picker_available);
        assert_eq!(app.detail_scope, selected);
        assert_eq!(app.detail_scroll, 3);
        app.open_date_picker();
        assert!(app.date_picker.is_none());
        terminal.backend_mut().resize(140, 40);
        terminal
            .resize(ratatui::layout::Rect::new(0, 0, 140, 40))
            .unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();
        assert_eq!(app.detail_scope, selected);
        assert!(app.date_picker_available);
    }

    #[test]
    fn daily_zero_size_and_no_matching_session_cannot_open_modal() {
        use crate::ui::app::daily_test_snapshot;
        for (width, height) in [(0, 0), (1, 1), (20, 6)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(daily_test_snapshot([true; 3]));
            terminal.draw(|f| render(f, &mut app)).unwrap();
            app.open_date_picker();
            assert!(app.date_picker.is_none());
        }
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.start_search();
        app.push_search_char('#');
        app.footer_notice = Some("notice hidden while editing".into());
        terminal.draw(|f| render(f, &mut app)).unwrap();
        app.open_date_picker();
        assert!(app.date_picker.is_none());
        let screen = daily_screen(&terminal);
        assert!(screen.contains("No matching sessions") && screen.contains("Search:"));
        assert!(!screen.contains("notice hidden"));
    }

    #[test]
    fn daily_narrow_notice_keeps_core_hints_across_noop_navigation() {
        use crate::ui::app::{DetailScope, daily_test_snapshot};
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.detail_scope = DetailScope::Day(app.snapshot.sessions[0].daily[0].day.unwrap());
        app.replace_snapshot(daily_test_snapshot([true, false, true]));
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        for _ in 0..2 {
            terminal.draw(|f| render(f, &mut app)).unwrap();
            let screen = daily_screen(&terminal);
            for hint in ["Date gone; All", "d date", "r reload", "q quit"] {
                assert!(screen.contains(hint), "missing {hint}: {screen}");
            }
            assert_eq!(app.detail_scope, DetailScope::All);
            app.move_down();
        }
        app.replace_snapshot(daily_test_snapshot([false; 3]));
        terminal.draw(|f| render(f, &mut app)).unwrap();
        let screen = daily_screen(&terminal);
        assert!(
            screen.contains("Session lost; All")
                && screen.contains("r reload")
                && screen.contains("q quit")
        );
        assert!(!screen.contains("d date"));
    }
}
