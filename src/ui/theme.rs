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

#[allow(dead_code)]
impl Theme {
    pub fn tab_active(&self) -> Style {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
    pub fn tab_inactive(&self) -> Style {
        Style::default().fg(Color::Gray)
    }
    pub fn selected_row_focused(&self) -> Style {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
    pub fn selected_row_unfocused(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
    pub fn delta_positive(&self) -> Style {
        Style::default().fg(Color::Yellow)
    }
    pub fn delta_negative(&self) -> Style {
        Style::default().fg(Color::Green)
    }
    pub fn filter_badge(&self) -> Style {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }
    pub fn hint_key(&self) -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }
    pub fn hint_desc(&self) -> Style {
        Style::default().fg(Color::Gray)
    }
    pub fn chart_bar(&self) -> Style {
        Style::default().fg(Color::Cyan)
    }
    pub fn chart_peak(&self) -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }
    pub fn border_focused(&self) -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_styles_are_distinct_and_ansi_safe() {
        let theme = Theme::default();
        assert_ne!(theme.tab_active(), theme.tab_inactive());
        assert_ne!(theme.selected_row_focused(), theme.selected_row_unfocused());
        assert_ne!(theme.delta_positive(), theme.delta_negative());
        assert_ne!(theme.border(), theme.border_focused());
        assert_ne!(theme.hint_key(), theme.hint_desc());
        assert_ne!(theme.chart_bar(), theme.chart_peak());
        assert_eq!(theme.filter_badge().bg, Some(Color::Yellow));
    }
}
