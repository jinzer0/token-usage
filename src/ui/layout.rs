use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LayoutMode {
    TooSmall,
    Narrow,
    Medium,
    Wide,
}

#[derive(Debug, Clone, Copy)]
pub struct AppLayout {
    pub mode: LayoutMode,
    pub header: Rect,
    pub sessions: Option<Rect>,
    pub detail: Rect,
    pub footer: Rect,
}

pub fn mode_for(width: u16, height: u16) -> LayoutMode {
    if width < 40 || height < 10 {
        LayoutMode::TooSmall
    } else if width < 80 {
        LayoutMode::Narrow
    } else if width < 110 {
        LayoutMode::Medium
    } else {
        LayoutMode::Wide
    }
}

pub fn calculate(area: Rect) -> AppLayout {
    let mode = mode_for(area.width, area.height);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    let header = vertical[0];
    let body = vertical[1];
    let footer = vertical[2];

    match mode {
        LayoutMode::TooSmall | LayoutMode::Narrow => AppLayout {
            mode,
            header,
            sessions: None,
            detail: body,
            footer,
        },
        LayoutMode::Medium => {
            let horizontal = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(26), Constraint::Min(20)])
                .split(body);
            AppLayout {
                mode,
                header,
                sessions: Some(horizontal[0]),
                detail: horizontal[1],
                footer,
            }
        }
        LayoutMode::Wide => {
            let horizontal = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(33), Constraint::Percentage(67)])
                .split(body);
            AppLayout {
                mode,
                header,
                sessions: Some(horizontal[0]),
                detail: horizontal[1],
                footer,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_breakpoints() {
        assert_eq!(mode_for(40, 10), LayoutMode::Narrow);
        assert_eq!(mode_for(60, 15), LayoutMode::Narrow);
        assert_eq!(mode_for(80, 24), LayoutMode::Medium);
        assert_eq!(mode_for(100, 30), LayoutMode::Medium);
        assert_eq!(mode_for(140, 40), LayoutMode::Wide);
    }
}
