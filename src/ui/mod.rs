mod app;
mod event;
mod format;
mod layout;
mod theme;
mod widgets;

use anyhow::Result;
use crossterm::{
    event::{self as ct_event, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use std::{io, time::Duration};

pub use app::App;

use crate::{aggregate::AggregateSnapshot, domain::SessionKey};

pub fn run<F>(snapshot: AggregateSnapshot, reload: F) -> Result<()>
where
    F: FnMut() -> Result<AggregateSnapshot>,
{
    run_with_initial(snapshot, None, reload)
}

pub fn run_with_initial<F>(
    snapshot: AggregateSnapshot,
    initial: Option<SessionKey>,
    mut reload: F,
) -> Result<()>
where
    F: FnMut() -> Result<AggregateSnapshot>,
{
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut app = App::with_initial_session(snapshot, initial);
    let mut last_area = Rect::default();

    loop {
        terminal.draw(|frame| {
            last_area = frame.area();
            widgets::render(frame, &mut app);
        })?;
        if !ct_event::poll(Duration::from_millis(200))? {
            continue;
        }
        match ct_event::read()? {
            Event::Key(key) if event::handle_key(&mut app, key, &mut reload)? => break,
            Event::Mouse(mouse) => event::handle_mouse(&mut app, mouse, last_area),
            _ => {}
        }
    }
    Ok(())
}

pub struct TerminalGuard;

impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}
