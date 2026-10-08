use super::app::{App, SearchMode};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn handle_key<F>(app: &mut App, key: KeyEvent, reload: &mut F) -> Result<bool>
where
    F: FnMut() -> Result<crate::aggregate::AggregateSnapshot>,
{
    if app.date_picker.is_some() {
        match key.code {
            KeyCode::Down => app.move_date_cursor(1),
            KeyCode::Up => app.move_date_cursor(-1),
            KeyCode::Enter => app.apply_date_picker(),
            KeyCode::Esc => app.date_picker = None,
            _ => {}
        }
        return Ok(false);
    }
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
        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
        KeyCode::Char('J') => app.scroll_down(),
        KeyCode::Char('K') => app.scroll_up(),
        KeyCode::PageDown => app.page_down(),
        KeyCode::PageUp => app.page_up(),
        KeyCode::Char('v') => app.toggle_breakdown(),
        KeyCode::Char('d') => app.open_date_picker(),
        KeyCode::Char('/') => app.start_search(),
        KeyCode::Char('?') => app.help_visible = true,
        KeyCode::Char('r') => match reload() {
            Ok(snapshot) => app.replace_snapshot(snapshot),
            Err(e) => app.footer_error = Some(format!("reload failed: {e}")),
        },
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{aggregate::aggregate, sources};
    use std::path::Path;

    #[test]
    fn failed_refresh_keeps_previous_usage_and_success_clears_error() {
        let original = aggregate(vec![
            sources::gjc::parse(Path::new("tests/fixtures/gjc")).unwrap(),
        ]);
        let before = serde_json::to_value(&original).unwrap();
        let mut app = App::new(original);
        let refresh = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
        let mut failed =
            || anyhow::bail!("token aggregate overflow in total_tokens with OpenCode selected");
        assert!(!handle_key(&mut app, refresh, &mut failed).unwrap());
        assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), before);
        assert!(
            app.footer_error
                .as_ref()
                .unwrap()
                .contains("token aggregate overflow")
        );
        let replacement = aggregate(vec![
            sources::codex::parse(Path::new("tests/fixtures/codex")).unwrap(),
        ]);
        let after = serde_json::to_value(&replacement).unwrap();
        let mut succeeded = || Ok(replacement.clone());
        assert!(!handle_key(&mut app, refresh, &mut succeeded).unwrap());
        assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), after);
        assert!(app.footer_error.is_none());
    }

    #[test]
    fn daily_modal_blocks_quit_reload_search_and_applies_only_enter() {
        use crate::ui::app::{DetailScope, daily_test_snapshot};
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.update_picker_availability(true);
        let mut calls = 0;
        let mut reload = || {
            calls += 1;
            Ok(daily_test_snapshot([true; 3]))
        };
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        handle_key(&mut app, key(KeyCode::Char('d')), &mut reload).unwrap();
        for ch in ['q', 'r', '/', 'v', '?', 'j', 'k', 'd'] {
            assert!(!handle_key(&mut app, key(KeyCode::Char(ch)), &mut reload).unwrap());
        }
        assert!(!app.breakdown && !app.help_visible);
        assert_eq!(app.search.mode, SearchMode::Inactive);
        handle_key(&mut app, key(KeyCode::Down), &mut reload).unwrap();
        assert_eq!(app.detail_scope, DetailScope::All);
        assert!(!handle_key(&mut app, key(KeyCode::Esc), &mut reload).unwrap());
        assert!(app.date_picker.is_none());
        assert_eq!(app.detail_scope, DetailScope::All);
        handle_key(&mut app, key(KeyCode::Char('d')), &mut reload).unwrap();
        handle_key(&mut app, key(KeyCode::Down), &mut reload).unwrap();
        handle_key(&mut app, key(KeyCode::Enter), &mut reload).unwrap();
        assert!(matches!(app.detail_scope, DetailScope::Day(_)));
        assert!(app.date_picker.is_none());
        assert_eq!(calls, 0);
    }

    #[test]
    fn daily_search_help_and_unavailable_date_opening_keep_existing_routing() {
        use crate::ui::app::daily_test_snapshot;
        let mut app = App::new(daily_test_snapshot([true; 3]));
        let mut reload = || anyhow::bail!("unexpected reload");
        let d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
        handle_key(&mut app, d, &mut reload).unwrap();
        assert!(app.date_picker.is_none());
        app.update_picker_availability(true);
        app.help_visible = true;
        handle_key(&mut app, d, &mut reload).unwrap();
        assert!(app.date_picker.is_none());
        app.help_visible = false;
        app.start_search();
        handle_key(&mut app, d, &mut reload).unwrap();
        assert_eq!(app.search.query, "d");
        assert!(app.date_picker.is_none());
    }

    #[test]
    fn daily_failed_reload_preserves_internal_data_scope_and_scroll() {
        use crate::ui::app::{DetailScope, daily_test_snapshot};
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.detail_scope = DetailScope::Undated;
        app.detail_scroll = 4;
        let before_key = app.selected_session.clone();
        let before = app
            .selected_session()
            .unwrap()
            .daily
            .iter()
            .map(|d| {
                (
                    d.day,
                    d.tokens.clone(),
                    d.record_count,
                    serde_json::to_value(&d.models).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let mut reload = || anyhow::bail!("controlled fixture read failure");
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            &mut reload,
        )
        .unwrap();
        let after = app
            .selected_session()
            .unwrap()
            .daily
            .iter()
            .map(|d| {
                (
                    d.day,
                    d.tokens.clone(),
                    d.record_count,
                    serde_json::to_value(&d.models).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(after, before);
        assert_eq!(app.selected_session, before_key);
        assert_eq!(app.detail_scope, DetailScope::Undated);
        assert_eq!(app.detail_scroll, 4);
        assert!(
            app.footer_error
                .as_ref()
                .unwrap()
                .contains("controlled fixture")
        );
    }
}
