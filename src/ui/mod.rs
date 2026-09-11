use crate::{aggregate::AggregateSnapshot, format};
use anyhow::Result;
use crossterm::{event::{self, Event, KeyCode}, execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};

pub struct AppState {
    pub snapshot: AggregateSnapshot,
    pub selected: usize,
    pub verbose: bool,
    pub footer_error: Option<String>,
}

impl AppState {
    pub fn new(snapshot: AggregateSnapshot) -> Self { Self { snapshot, selected: 0, verbose: false, footer_error: None } }
    pub fn down(&mut self) { if self.selected + 1 < self.snapshot.sessions.len() { self.selected += 1; } }
    pub fn up(&mut self) { self.selected = self.selected.saturating_sub(1); }
}

pub fn run<F>(snapshot: AggregateSnapshot, mut reload: F) -> Result<()>
where F: FnMut() -> Result<AggregateSnapshot> {
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut app = AppState::new(snapshot);
    loop {
        terminal.draw(|frame| render_frame(frame, &app))?;
        if !event::poll(Duration::from_millis(200))? { continue; }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down | KeyCode::Char('j') => app.down(),
                KeyCode::Up | KeyCode::Char('k') => app.up(),
                KeyCode::Char('v') => app.verbose = !app.verbose,
                KeyCode::Char('r') => match reload() { Ok(snapshot) => { app.snapshot = snapshot; app.selected = app.selected.min(app.snapshot.sessions.len().saturating_sub(1)); app.footer_error = None; }, Err(e) => app.footer_error = Some(e.to_string()) },
                _ => {}
            }
        }
    }
    Ok(())
}

fn render_frame(frame: &mut ratatui::Frame<'_>, app: &AppState) {
    use ratatui::{layout::{Constraint, Direction, Layout}, text::Line, widgets::{Block, Borders, Paragraph}};
    let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(3), Constraint::Min(3), Constraint::Length(2)]).split(frame.area());
    frame.render_widget(Paragraph::new("token-usage TUI").block(Block::new().borders(Borders::ALL)), chunks[0]);
    let text = if app.snapshot.sessions.is_empty() {
        "No usage records found.".to_string()
    } else {
        let session = &app.snapshot.sessions[app.selected];
        format::plain::detail(session, app.verbose)
    };
    frame.render_widget(Paragraph::new(text).block(Block::new().borders(Borders::ALL).title("detail")), chunks[1]);
    let footer = app.footer_error.clone().unwrap_or_else(|| "q quit · j/k move · v verbose · r reload".into());
    frame.render_widget(Paragraph::new(Line::from(footer)), chunks[2]);
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
