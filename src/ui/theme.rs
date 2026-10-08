use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone)]
pub struct Theme;

impl Theme {
    pub fn default() -> Self {
        Self
    }

    pub fn header(&self) -> Style {
        Style::default().fg(Color::Gray)
    }
    pub fn title(&self) -> Style {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    }
    pub fn primary_text(&self) -> Style {
        Style::default().fg(Color::White)
    }
    pub fn muted_text(&self) -> Style {
        Style::default().fg(Color::DarkGray)
    }
    pub fn number(&self) -> Style {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    }
    pub fn percentage(&self) -> Style {
        Style::default().fg(Color::Gray)
    }
    pub fn reasoning(&self) -> Style {
        Style::default().fg(Color::Cyan)
    }
    pub fn border(&self) -> Style {
        Style::default().fg(Color::DarkGray)
    }
    pub fn selected_row(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
    pub fn model_name(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
    pub fn key_hint(&self) -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }
    pub fn warning(&self) -> Style {
        Style::default().fg(Color::Yellow)
    }
    pub fn error(&self) -> Style {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    }
    pub fn client_codex(&self) -> Style {
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    }
    pub fn client_gjc(&self) -> Style {
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
    }
    pub fn client_opencode(&self) -> Style {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    }
    pub fn bar_full(&self) -> Style {
        Style::default().fg(Color::Cyan)
    }
}
