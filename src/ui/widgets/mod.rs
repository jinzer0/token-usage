mod date_list;
mod footer;
mod header;
mod help;
mod session_detail;
mod session_list;

use crate::ui::{
    app::{App, ViewTab},
    layout,
    theme::Theme,
};
use ratatui::{Frame, widgets::Paragraph};

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    let theme = Theme::default();
    let area = frame.area();
    let layout = layout::calculate(area, app.tab);
    if layout.mode == layout::LayoutMode::TooSmall {
        let message = if app.tab == ViewTab::Dates {
            "Enlarge terminal for Dates\nMinimum: 110x12 (three panes)\n1 Sessions · q quit"
        } else {
            "Enlarge terminal\nMinimum: 40x10\n2 Dates needs 110x12 · q quit"
        };
        frame.render_widget(Paragraph::new(message), area);
        return;
    }
    header::render(frame, layout.header, app, &theme);
    if let Some(dates) = layout.dates {
        date_list::render(frame, dates, app, &theme);
    }
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
        aggregate::aggregate,
        domain::{Client, ReasoningEffort, SessionId, SessionKey, TokenStats, UsageRecord},
        scan::ParseResult,
    };
    use ratatui::{Terminal, backend::TestBackend};

    fn screen(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn draw(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        terminal
    }

    fn source_snapshot() -> crate::aggregate::AggregateSnapshot {
        let mut result = ParseResult::empty(Client::Gjc, "render-fixture".into());
        for (client, name, model) in [
            (Client::OpenCode, "opencode-root", "claude-opencode"),
            (Client::Codex, "codex-root", "gpt-codex"),
            (Client::Gjc, "gjc-root", "gpt-gjc"),
        ] {
            result.records.push(UsageRecord {
                session_key: SessionKey {
                    client,
                    id: SessionId("same".into()),
                },
                parent_session_key: None,
                message_id: None,
                source_path: "render-fixture".into(),
                source_line: None,
                started_at: None,
                session_name: Some(name.into()),
                model: model.into(),
                reasoning_effort: Some(ReasoningEffort::Custom("xhigh".into())),
                tokens: TokenStats {
                    total_tokens: 1000,
                    input_total: 700,
                    output_total: 300,
                    reasoning_known: 120,
                    reasoning_has_unknown: true,
                    ..Default::default()
                },
            });
        }
        aggregate(vec![result]).unwrap()
    }

    #[test]
    fn sessions_keep_client_model_effort_reasoning_and_all_totals() {
        let mut app = App::new(source_snapshot());
        app.selected_session = Some(SessionKey {
            client: Client::OpenCode,
            id: SessionId("same".into()),
        });
        app.rebuild_filter();
        let text = screen(&draw(&mut app, 180, 40));
        for expected in [
            "1 Sessions",
            "2 Dates",
            "[O] opencode-root",
            "[C] codex-root",
            "[G] gjc-root",
            "OPENCODE",
            "claude-opencode",
            "xhigh",
            "reasoning",
            "120+",
            "Total 1000",
            "codex 1 · gjc 1 · opencode 1",
            "Last used",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        app.breakdown = true;
        let text = screen(&draw(&mut app, 180, 40));
        assert!(text.contains("INPUT") && text.contains("REASONING"));
        assert!(text.contains("Cache write 0") && text.contains("some unknown"));
    }

    #[test]
    fn dates_minimum_three_panes_and_scoped_detail_are_visible() {
        let mut app = App::new(crate::ui::app::daily_test_snapshot([true; 3]));
        app.switch_tab(ViewTab::Dates);
        let text = screen(&draw(&mut app, 110, 12));
        for expected in [
            "Dates · All sessions",
            "Sessions · Last used",
            "day-new",
            "Total 300",
            "Δ prev usage day",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(!text.contains("day-old") && !text.contains("day-unknown"));
        let scope = app.selected_date;
        let session = app.selected_session.clone();
        for (width, height) in [(109, 12), (110, 11), (40, 12)] {
            let text = screen(&draw(&mut app, width, height));
            assert!(text.contains("Enlarge terminal for Dates") && text.contains("110x12"));
            assert!(!text.contains("day-new"));
            assert_eq!(app.selected_date, scope);
            assert_eq!(app.selected_session, session);
        }
        let text = screen(&draw(&mut app, 140, 30));
        assert!(text.contains("day-new"));
        app.move_date(1);
        let text = screen(&draw(&mut app, 140, 30));
        assert!(text.contains("day-old") && text.contains("Total 100"));
        assert!(!text.contains("day-new"));
        app.move_date(1);
        let text = screen(&draw(&mut app, 140, 30));
        assert!(text.contains("Unknown date") && text.contains("day-unknown"));
    }

    #[test]
    fn pin_resize_search_and_help_keep_honest_scope_labels() {
        let mut app = App::new(crate::ui::app::daily_test_snapshot([true; 3]));
        app.open_session_dates();
        let key = app.pinned_session.clone();
        let text = screen(&draw(&mut app, 140, 30));
        assert!(text.contains("Dates · Pinned") && text.contains("all 300"));
        draw(&mut app, 20, 6);
        assert_eq!(app.pinned_session, key);
        app.start_search();
        app.push_search_char('#');
        let text = screen(&draw(&mut app, 140, 30));
        assert!(text.contains("No matching sessions") && text.contains("Search:"));
        assert!(text.contains("Dates · All sessions"));
        app.cancel_search();
        app.help_visible = true;
        let text = screen(&draw(&mut app, 110, 12));
        for expected in [
            "Sessions / Dates tabs",
            "previous usage day",
            "selected day in Dates",
            "close help",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
    }

    #[test]
    fn exact_extreme_totals_and_unicode_names_survive_minimum_width() {
        let mut result = ParseResult::empty(Client::Gjc, "extreme".into());
        for (at, total) in [
            ("2026-03-11T12:00:00Z", u64::MAX - 9),
            ("2026-03-10T12:00:00Z", 9),
        ] {
            result.records.push(UsageRecord {
                session_key: SessionKey {
                    client: Client::Gjc,
                    id: SessionId("long-id".repeat(20)),
                },
                parent_session_key: None,
                message_id: None,
                source_path: "extreme".into(),
                source_line: None,
                started_at: Some(at.parse().unwrap()),
                session_name: Some("한국어 세션 이름이 매우 긴 테스트 세션".into()),
                model: "原래 모델명".repeat(12),
                reasoning_effort: None,
                tokens: TokenStats {
                    total_tokens: total,
                    ..Default::default()
                },
            });
        }
        let mut app = App::new(aggregate(vec![result]).unwrap());
        app.switch_tab(ViewTab::Dates);
        let terminal = draw(&mut app, 110, 12);
        let text = screen(&terminal);
        assert!(text.contains(&format!("Total {}", u64::MAX - 9)));
        let dates = terminal
            .backend()
            .buffer()
            .content()
            .chunks(110)
            .flat_map(|row| row[..24].iter().map(|cell| cell.symbol()))
            .collect::<String>();
        assert!(
            dates.contains(&crate::ui::format::token_count(u64::MAX - 9)),
            "clipped date total: {dates}"
        );
        assert!(
            dates.contains(&format!(
                "+{}",
                crate::ui::format::token_count(u64::MAX - 18)
            )),
            "clipped delta: {dates}"
        );
        assert!(text.contains("Δ prev usage day") && text.contains('…'));
    }

    #[test]
    fn raw_model_effort_and_cache_values_are_scrollable_at_three_pane_minimum() {
        let mut result = ParseResult::empty(Client::Gjc, "exact-fields".into());
        for (total, effort, unknown) in [
            (1001, ReasoningEffort::Low, false),
            (1002, ReasoningEffort::High, true),
        ] {
            result.records.push(UsageRecord {
                session_key: SessionKey {
                    client: Client::Gjc,
                    id: SessionId("raw-session".into()),
                },
                parent_session_key: None,
                message_id: None,
                source_path: "exact-fields".into(),
                source_line: None,
                started_at: Some("2026-03-11T12:00:00Z".parse().unwrap()),
                session_name: None,
                model: "same-model".into(),
                reasoning_effort: Some(effort),
                tokens: TokenStats {
                    total_tokens: total,
                    input_total: 9_000_000_030,
                    input_uncached: 29,
                    cache_read: 9_000_000_001,
                    cache_write: 7,
                    output_total: 17,
                    reasoning_known: 3,
                    reasoning_has_unknown: unknown,
                },
            });
        }
        let mut app = App::new(aggregate(vec![result]).unwrap());
        app.switch_tab(ViewTab::Dates);
        app.breakdown = true;
        let mut observed = String::new();
        for scroll in 0..80 {
            app.detail_scroll = scroll;
            observed.push_str(&screen(&draw(&mut app, 110, 12)));
        }
        for exact in [
            "Model exact total 2003",
            "Effort low exact total 1001",
            "Effort high exact total 1002",
            "INPUT 9000000030",
            "Uncached 29",
            "Cache read 9000000001",
            "Cache write 7",
            "OUTPUT 17",
            "REASONING 3+ (some unknown)",
        ] {
            assert!(observed.contains(exact), "missing exact field {exact}");
        }
    }

    #[test]
    fn empty_zero_size_errors_and_notice_do_not_mutate_selection() {
        let mut app = App::new(aggregate(vec![]).unwrap());
        for (width, height) in [(0, 0), (1, 1), (20, 6), (40, 10), (110, 12)] {
            draw(&mut app, width, height);
            assert!(app.selected_session.is_none());
        }
        app.switch_tab(ViewTab::Dates);
        let text = screen(&draw(&mut app, 110, 12));
        assert!(text.contains("No usage sessions"));
        app.footer_error = Some("token aggregate overflow in total_tokens".into());
        let text = screen(&draw(&mut app, 140, 30));
        assert!(
            text.contains("ERROR") && text.contains("r retry") && text.contains("total_tokens")
        );
        app.footer_error = None;
        app.footer_notice = Some("Date gone; selection recovered".into());
        let text = screen(&draw(&mut app, 110, 12));
        assert!(text.contains("Date gone") && text.contains("q quit"));
    }
}
