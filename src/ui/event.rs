use super::app::{App, SearchMode};
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
        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
        KeyCode::Char('J') => app.scroll_down(),
        KeyCode::Char('K') => app.scroll_up(),
        KeyCode::PageDown => app.page_down(),
        KeyCode::PageUp => app.page_up(),
        KeyCode::Char('v') => app.toggle_breakdown(),
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
