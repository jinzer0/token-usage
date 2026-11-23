use super::{
    app::{App, PaneFocus, SearchMode, ViewTab},
    layout,
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

pub fn handle_key<F>(app: &mut App, key: KeyEvent, reload: &mut F) -> Result<bool>
where
    F: FnMut() -> Result<crate::aggregate::AggregateSnapshot>,
{
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }
    if app.help_visible {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => app.help_visible = false,
            _ => {}
        }
        return Ok(false);
    }
    if app.period_input.editing {
        match key.code {
            KeyCode::Esc => app.cancel_period_input(),
            KeyCode::Enter => app.confirm_period_input(),
            KeyCode::Backspace => app.pop_period_digit(),
            KeyCode::Char(ch)
                if ch.is_ascii_digit() && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                app.push_period_digit(ch)
            }
            _ => {}
        }
        return Ok(false);
    }
    if app.search.mode == SearchMode::Editing {
        match key.code {
            KeyCode::Esc => app.cancel_search(),
            KeyCode::Enter => app.confirm_search(),
            KeyCode::Backspace => app.pop_search_char(),
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.push_search_char(ch)
            }
            _ => {}
        }
        return Ok(false);
    }
    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Esc => {
            if app.search.mode == SearchMode::Confirmed || !app.search.query.is_empty() {
                app.cancel_search();
            } else {
                return Ok(true);
            }
        }
        KeyCode::Char('1') => app.switch_tab(ViewTab::Sessions),
        KeyCode::Char('2') => app.switch_tab(ViewTab::Dates),
        KeyCode::Char('3') => app.switch_tab(ViewTab::Models),
        KeyCode::Char('c') => app.cycle_client_filter(),
        KeyCode::Char('f') => app.cycle_period_filter(),
        KeyCode::Char('F') | KeyCode::Char('n') => app.start_period_input(),
        KeyCode::BackTab | KeyCode::Left => app.move_focus(-1),
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => app.move_focus(-1),
        KeyCode::Tab | KeyCode::Right | KeyCode::Enter => app.move_focus(1),
        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
        KeyCode::Char('g') | KeyCode::Home => app.move_first(),
        KeyCode::Char('G') | KeyCode::End => app.move_last(),
        KeyCode::Char('[') => app.move_date(-1),
        KeyCode::Char(']') => app.move_date(1),
        KeyCode::Char('J') => app.scroll_down(),
        KeyCode::Char('K') => app.scroll_up(),
        KeyCode::PageDown => app.page_down(),
        KeyCode::PageUp => app.page_up(),
        KeyCode::Char('v') => app.toggle_breakdown(),
        KeyCode::Char('d') => app.open_session_dates(),
        KeyCode::Char('p') => app.toggle_pin(),
        KeyCode::Char('s') => app.toggle_sort(),
        KeyCode::Char('/') => app.start_search(),
        KeyCode::Char('?') => app.help_visible = true,
        KeyCode::Char('r') => match reload() {
            Ok(snapshot) => app.replace_snapshot(snapshot),
            Err(e) => app.footer_error = Some(format!("reload failed: {e:#}")),
        },
        _ => {}
    }
    Ok(false)
}

pub fn handle_mouse(app: &mut App, mouse: MouseEvent, area: Rect) {
    if app.help_visible {
        return;
    }
    let l = layout::calculate(area, app.tab);
    if l.mode == layout::LayoutMode::TooSmall {
        return;
    }
    let col = mouse.column;
    let row = mouse.row;
    match mouse.kind {
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let delta = if mouse.kind == MouseEventKind::ScrollDown {
                1
            } else {
                -1
            };
            if l.dates.is_some_and(|r| contains(r, col, row)) {
                app.move_date(delta);
            } else if l.sessions.is_some_and(|r| contains(r, col, row)) {
                app.move_selection(delta);
            } else if l.models.is_some_and(|r| contains(r, col, row)) {
                app.move_model(delta);
            } else if contains(l.detail, col, row) {
                if delta > 0 {
                    app.scroll_down();
                } else {
                    app.scroll_up();
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if contains(l.header, col, row) && row == l.header.y {
                let rel_x = col.saturating_sub(l.header.x);
                if (14..26).contains(&rel_x) {
                    app.switch_tab(ViewTab::Sessions);
                } else if (27..36).contains(&rel_x) {
                    app.switch_tab(ViewTab::Dates);
                } else if (37..47).contains(&rel_x) {
                    app.switch_tab(ViewTab::Models);
                }
                return;
            }
            if let Some(r) = l.dates
                && contains(r, col, row)
            {
                app.focus = PaneFocus::Dates;
                if inner_contains(r, col, row) {
                    let rel_y = (row - r.y - 1) as usize;
                    let inner_width = r.width.saturating_sub(2) as usize;
                    if let Some(pos) = date_row_at(app, rel_y, inner_width) {
                        app.select_date_at(pos);
                    }
                }
            } else if let Some(r) = l.sessions
                && contains(r, col, row)
            {
                app.focus = PaneFocus::Sessions;
                if inner_contains(r, col, row) {
                    let pos = app.session_list_state.offset() + (row - r.y - 1) as usize;
                    app.select_session_at(pos);
                }
            } else if let Some(r) = l.models
                && contains(r, col, row)
            {
                app.focus = PaneFocus::Models;
                if inner_contains(r, col, row) {
                    let pos = app.model_list_state.offset() + (row - r.y - 1) as usize;
                    app.select_model_at(pos);
                }
            } else if contains(l.detail, col, row) {
                app.focus = PaneFocus::Details;
            }
        }
        _ => {}
    }
}

fn contains(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.right() && row >= rect.y && row < rect.bottom()
}

fn inner_contains(rect: Rect, col: u16, row: u16) -> bool {
    col > rect.x && col + 1 < rect.right() && row > rect.y && row + 1 < rect.bottom()
}

fn date_row_at(app: &App, rel_y: usize, inner_width: usize) -> Option<usize> {
    let pinned = app.pinned_session.is_some();
    let mut y = 0usize;
    for pos in app.date_list_state.offset()..app.visible_dates.len() {
        let index = app.visible_dates[pos];
        let date = app.snapshot.dates.get(index)?;
        let Some(tokens) = app.date_tokens(index) else {
            continue;
        };
        let mut height = 2usize;
        if date.day.is_some()
            && let Some(previous) = app
                .visible_dates
                .get(pos + 1)
                .and_then(|prev| {
                    app.snapshot
                        .dates
                        .get(*prev)
                        .and_then(|d| d.day)
                        .map(|_| *prev)
                })
                .and_then(|prev| app.date_tokens(prev))
        {
            let delta = i128::from(tokens.total_tokens) - i128::from(previous.total_tokens);
            let magnitude = delta.unsigned_abs() as u64;
            let delta_str = if delta > 0 {
                format!("+{}", super::format::token_count(magnitude))
            } else if delta < 0 {
                format!("-{}", super::format::token_count(magnitude))
            } else {
                "+0".into()
            };
            let total_str = super::format::token_count(tokens.total_tokens);
            let one_space_len = 2 + total_str.len() + 1 + delta_str.len();
            if one_space_len > inner_width {
                height += 1;
            }
        }
        if pinned {
            height += 1;
        }
        if rel_y < y + height {
            return Some(pos);
        }
        y += height;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::{ClientScope, PeriodFilter, SessionSort, daily_test_snapshot};
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    #[test]
    fn refresh_failure_keeps_snapshot_navigation_and_success_clears_error() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.open_session_dates();
        app.move_date(1);
        app.toggle_sort();
        app.detail_scroll = 4;
        let before = serde_json::to_value(&app.snapshot).unwrap();
        let before_dates = app
            .snapshot
            .dates
            .iter()
            .map(|d| (d.day, d.tokens.clone(), d.record_count))
            .collect::<Vec<_>>();
        let selection = (
            app.selected_session.clone(),
            app.selected_date,
            app.pinned_session.clone(),
        );
        let mut failed = || {
            Err(anyhow::anyhow!("overflow in total_tokens").context("controlled fixture failure"))
        };
        handle_key(&mut app, key(KeyCode::Char('r')), &mut failed).unwrap();
        assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), before);
        assert_eq!(
            app.snapshot
                .dates
                .iter()
                .map(|d| (d.day, d.tokens.clone(), d.record_count))
                .collect::<Vec<_>>(),
            before_dates
        );
        assert_eq!(
            (
                app.selected_session.clone(),
                app.selected_date,
                app.pinned_session.clone()
            ),
            selection
        );
        assert_eq!(app.tab, ViewTab::Dates);
        assert_eq!(app.sort, SessionSort::Tokens);
        assert_eq!(app.detail_scroll, 4);
        assert!(
            app.footer_error
                .as_ref()
                .unwrap()
                .contains("controlled fixture failure")
        );
        assert!(
            app.footer_error
                .as_ref()
                .unwrap()
                .contains("overflow in total_tokens")
        );
        let mut success = || Ok(daily_test_snapshot([true; 3]));
        handle_key(&mut app, key(KeyCode::Char('r')), &mut success).unwrap();
        assert!(app.footer_error.is_none());
        assert_eq!(app.detail_scroll, 4);
        assert!(
            app.footer_notice
                .as_deref()
                .is_some_and(|n| n.starts_with("Reloaded at "))
        );
    }
    #[test]
    fn direct_tabs_focus_date_and_detail_routes_have_no_modal() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        let mut reload = || anyhow::bail!("unexpected reload");
        handle_key(&mut app, key(KeyCode::Char('2')), &mut reload).unwrap();
        assert_eq!(app.focus, PaneFocus::Dates);
        let first = app.selected_date;
        handle_key(&mut app, key(KeyCode::Char('j')), &mut reload).unwrap();
        assert_ne!(app.selected_date, first);
        handle_key(&mut app, key(KeyCode::Enter), &mut reload).unwrap();
        assert_eq!(app.focus, PaneFocus::Sessions);
        handle_key(&mut app, key(KeyCode::Right), &mut reload).unwrap();
        assert_eq!(app.focus, PaneFocus::Details);
        handle_key(&mut app, key(KeyCode::Char('j')), &mut reload).unwrap();
        assert_eq!(app.detail_scroll, 1);
        handle_key(&mut app, key(KeyCode::Char('[')), &mut reload).unwrap();
        assert_eq!(app.selected_date, first);
        handle_key(&mut app, key(KeyCode::BackTab), &mut reload).unwrap();
        assert_eq!(app.focus, PaneFocus::Sessions);
        handle_key(&mut app, key(KeyCode::Char('s')), &mut reload).unwrap();
        assert_eq!(app.sort, SessionSort::Tokens);
        handle_key(&mut app, key(KeyCode::Char('1')), &mut reload).unwrap();
        assert_eq!(app.tab, ViewTab::Sessions);
        assert_eq!(app.sort, SessionSort::LastUsed);
        handle_key(&mut app, key(KeyCode::Char('3')), &mut reload).unwrap();
        assert_eq!(app.tab, ViewTab::Models);
        assert_eq!(app.focus, PaneFocus::Models);
        handle_key(&mut app, key(KeyCode::Char('d')), &mut reload).unwrap();
        assert_eq!(app.tab, ViewTab::Dates);
        assert!(app.pinned_session.is_some());
        handle_key(&mut app, key(KeyCode::Char('p')), &mut reload).unwrap();
        assert!(app.pinned_session.is_none());
        assert!(handle_key(&mut app, key(KeyCode::Esc), &mut reload).unwrap());
    }
    #[test]
    fn search_and_help_override_navigation_and_quit() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        let mut reload = || anyhow::bail!("unexpected reload");
        handle_key(&mut app, key(KeyCode::Char('/')), &mut reload).unwrap();
        for ch in ['1', '2', 's', 'd', 'p', 'q'] {
            assert!(!handle_key(&mut app, key(KeyCode::Char(ch)), &mut reload).unwrap());
        }
        assert_eq!(app.search.query, "12sdpq");
        assert_eq!(app.tab, ViewTab::Sessions);
        assert_eq!(app.sort, SessionSort::LastUsed);
        assert!(!handle_key(&mut app, key(KeyCode::Esc), &mut reload).unwrap());
        assert!(app.search.query.is_empty());
        handle_key(&mut app, key(KeyCode::Char('?')), &mut reload).unwrap();
        handle_key(&mut app, key(KeyCode::Char('2')), &mut reload).unwrap();
        assert_eq!(app.tab, ViewTab::Sessions);
        assert!(!handle_key(&mut app, key(KeyCode::Char('q')), &mut reload).unwrap());
        assert!(!app.help_visible);
    }
    #[test]
    fn esc_clears_confirmed_search_before_quitting_and_ctrl_c_quits() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        let mut reload = || anyhow::bail!("unexpected reload");
        handle_key(&mut app, key(KeyCode::Char('/')), &mut reload).unwrap();
        handle_key(&mut app, key(KeyCode::Char('x')), &mut reload).unwrap();
        handle_key(&mut app, key(KeyCode::Enter), &mut reload).unwrap();
        assert_eq!(app.search.mode, SearchMode::Confirmed);
        assert!(!handle_key(&mut app, key(KeyCode::Esc), &mut reload).unwrap());
        assert_eq!(app.search.mode, SearchMode::Inactive);
        assert!(app.search.query.is_empty());
        assert!(handle_key(&mut app, key(KeyCode::Esc), &mut reload).unwrap());

        assert!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &mut reload
            )
            .unwrap()
        );
    }
    #[test]
    fn client_period_quick_nav_and_mouse_events_work() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        let mut reload = || anyhow::bail!("unexpected reload");
        handle_key(&mut app, key(KeyCode::Char('c')), &mut reload).unwrap();
        assert_eq!(app.client_filter, ClientScope::Codex);
        handle_key(&mut app, key(KeyCode::Char('c')), &mut reload).unwrap();
        assert_eq!(app.client_filter, ClientScope::Gjc);
        handle_key(&mut app, key(KeyCode::Char('f')), &mut reload).unwrap();
        assert_eq!(app.period_filter, PeriodFilter::Today);
        handle_key(&mut app, key(KeyCode::Char('f')), &mut reload).unwrap();
        handle_key(&mut app, key(KeyCode::Char('f')), &mut reload).unwrap();
        handle_key(&mut app, key(KeyCode::Char('f')), &mut reload).unwrap();
        assert_eq!(app.period_filter, PeriodFilter::All);

        let area = Rect::new(0, 0, 120, 30);
        // Click "3 Models" in header (row 0, col 38)
        handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 38,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(app.tab, ViewTab::Models);
        assert_eq!(app.focus, PaneFocus::Models);

        // Mouse wheel down over detail pane (right side)
        handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 80,
                row: 15,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(app.detail_scroll, 1);

        // Quick nav g / G on models
        handle_key(&mut app, key(KeyCode::Char('G')), &mut reload).unwrap();
        assert_eq!(app.selected_model_position(), Some(2));
        handle_key(&mut app, key(KeyCode::Char('g')), &mut reload).unwrap();
        assert_eq!(app.selected_model_position(), Some(0));
    }

    #[test]
    fn custom_period_input_accepts_digits_applies_on_enter_and_cancels_on_esc() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        let mut reload = || anyhow::bail!("unexpected reload");

        // Trigger with 'n', ignore non-digits and navigation keys, apply 14d on Enter
        assert!(!handle_key(&mut app, key(KeyCode::Char('n')), &mut reload).unwrap());
        assert!(app.period_input.editing);
        for ch in ['q', 's', 'x', '1', '4'] {
            assert!(!handle_key(&mut app, key(KeyCode::Char(ch)), &mut reload).unwrap());
        }
        assert_eq!(app.period_input.query, "14");
        assert!(!handle_key(&mut app, key(KeyCode::Enter), &mut reload).unwrap());
        assert!(!app.period_input.editing);
        assert_eq!(app.period_filter, PeriodFilter::CustomDays(14));

        // Trigger with 'F' (pre-fills "14"), edit to "7", then Esc cancels without quitting
        assert!(!handle_key(&mut app, key(KeyCode::Char('F')), &mut reload).unwrap());
        assert_eq!(app.period_input.query, "14");
        assert!(!handle_key(&mut app, key(KeyCode::Backspace), &mut reload).unwrap());
        assert!(!handle_key(&mut app, key(KeyCode::Char('7')), &mut reload).unwrap());
        assert_eq!(app.period_input.query, "17");
        assert!(!handle_key(&mut app, key(KeyCode::Esc), &mut reload).unwrap());
        assert!(!app.period_input.editing);
        assert!(app.period_input.query.is_empty());
        assert_eq!(app.period_filter, PeriodFilter::CustomDays(14));
    }
}
