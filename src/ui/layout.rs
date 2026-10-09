use super::app::ViewTab;
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
    pub dates: Option<Rect>,
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

pub fn calculate(area: Rect, tab: ViewTab) -> AppLayout {
    let mode = match tab {
        ViewTab::Dates if area.width < 110 || area.height < 12 => LayoutMode::TooSmall,
        ViewTab::Dates => LayoutMode::Wide,
        ViewTab::Sessions => mode_for(area.width, area.height),
    };
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    let header = vertical[0];
    let body = vertical[1];
    let footer = vertical[2];
    if tab == ViewTab::Dates && mode != LayoutMode::TooSmall {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(24),
                Constraint::Length(30),
                Constraint::Min(56),
            ])
            .split(body);
        return AppLayout {
            mode,
            header,
            dates: Some(horizontal[0]),
            sessions: Some(horizontal[1]),
            detail: horizontal[2],
            footer,
        };
    }
    match mode {
        LayoutMode::TooSmall | LayoutMode::Narrow => AppLayout {
            mode,
            header,
            dates: None,
            sessions: None,
            detail: body,
            footer,
        },
        LayoutMode::Medium | LayoutMode::Wide => {
            let constraints = if mode == LayoutMode::Medium {
                [Constraint::Length(26), Constraint::Min(20)]
            } else {
                [Constraint::Percentage(33), Constraint::Percentage(67)]
            };
            let horizontal = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(constraints)
                .split(body);
            AppLayout {
                mode,
                header,
                dates: None,
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
    fn sessions_retain_breakpoints() {
        for (width, height, mode) in [
            (39, 10, LayoutMode::TooSmall),
            (40, 9, LayoutMode::TooSmall),
            (40, 10, LayoutMode::Narrow),
            (79, 12, LayoutMode::Narrow),
            (80, 12, LayoutMode::Medium),
            (109, 12, LayoutMode::Medium),
            (110, 12, LayoutMode::Wide),
        ] {
            assert_eq!(
                calculate(Rect::new(0, 0, width, height), ViewTab::Sessions).mode,
                mode
            );
        }
    }
    #[test]
    fn dates_require_all_three_panes_at_exact_boundary() {
        for (width, height) in [(109, 12), (110, 11)] {
            let layout = calculate(Rect::new(0, 0, width, height), ViewTab::Dates);
            assert_eq!(layout.mode, LayoutMode::TooSmall);
            assert!(layout.dates.is_none() && layout.sessions.is_none());
        }
        let layout = calculate(Rect::new(0, 0, 110, 12), ViewTab::Dates);
        assert_eq!(layout.mode, LayoutMode::Wide);
        assert_eq!(layout.header.height, 2);
        assert_eq!(layout.footer.height, 1);
        assert_eq!(layout.dates.unwrap(), Rect::new(0, 2, 24, 9));
        assert_eq!(layout.sessions.unwrap(), Rect::new(24, 2, 30, 9));
        assert_eq!(layout.detail, Rect::new(54, 2, 56, 9));
        let wide = calculate(Rect::new(0, 0, 140, 20), ViewTab::Dates);
        assert_eq!(wide.detail.width, 86);
    }
    #[test]
    fn zero_dimensions_are_safe_and_bounded() {
        for tab in [ViewTab::Sessions, ViewTab::Dates] {
            for (width, height) in [(0, 0), (0, 12), (110, 0), (1, 1)] {
                let area = Rect::new(0, 0, width, height);
                let layout = calculate(area, tab);
                assert_eq!(layout.mode, LayoutMode::TooSmall);
                for rect in [layout.header, layout.detail, layout.footer] {
                    assert!(rect.right() <= area.right() && rect.bottom() <= area.bottom());
                }
            }
        }
    }
}
