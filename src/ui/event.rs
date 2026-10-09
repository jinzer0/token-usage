use super::app::{App, SearchMode, ViewTab};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn handle_key<F>(app: &mut App, key: KeyEvent, reload: &mut F) -> Result<bool>
where
    F: FnMut() -> Result<crate::aggregate::AggregateSnapshot>,
{
    if app.help_visible {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => app.help_visible = false,
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
        KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
        KeyCode::Char('1') => app.switch_tab(ViewTab::Sessions),
        KeyCode::Char('2') => app.switch_tab(ViewTab::Dates),
        KeyCode::BackTab | KeyCode::Left => app.move_focus(-1),
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => app.move_focus(-1),
        KeyCode::Tab | KeyCode::Right | KeyCode::Enter => app.move_focus(1),
        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::{PaneFocus, SessionSort, daily_test_snapshot};
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
}
