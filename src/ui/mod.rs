mod app;
mod event;
mod format;
mod layout;
mod theme;
mod widgets;

use anyhow::Result;
use crossterm::{
    event::{self as ct_event, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{io, time::Duration};

pub use app::App;

use crate::aggregate::AggregateSnapshot;

pub fn run<F>(snapshot: AggregateSnapshot, mut reload: F) -> Result<()>
where
    F: FnMut() -> Result<AggregateSnapshot>,
{
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut app = App::new(snapshot);

    loop {
        terminal.draw(|frame| widgets::render(frame, &mut app))?;
        if !ct_event::poll(Duration::from_millis(200))? {
            continue;
        }
        if let Event::Key(key) = ct_event::read()?
            && event::handle_key(&mut app, key, &mut reload)?
        {
            break;
        }
    }
    Ok(())
}

pub struct TerminalGuard;

impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
