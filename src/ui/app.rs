use crate::{aggregate::AggregateSnapshot, domain::SessionKey};
use ratatui::widgets::ListState;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SearchMode {
    Inactive,
    Editing,
    Confirmed,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SearchState {
    pub mode: SearchMode,
    pub query: String,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            mode: SearchMode::Inactive,
            query: String::new(),
        }
    }
}

pub struct App {
    pub snapshot: AggregateSnapshot,
    pub session_list_state: ListState,
    pub selected_session: Option<SessionKey>,
    pub filtered_sessions: Vec<usize>,
    pub detail_scroll: usize,
    pub breakdown: bool,
    pub search: SearchState,
    pub help_visible: bool,
    pub footer_error: Option<String>,
}

impl App {
    pub fn new(snapshot: AggregateSnapshot) -> Self {
        let mut app = Self {
            snapshot,
            session_list_state: ListState::default(),
            selected_session: None,
            filtered_sessions: Vec::new(),
            detail_scroll: 0,
            breakdown: false,
            search: SearchState::default(),
            help_visible: false,
            footer_error: None,
        };
        app.rebuild_filter();
        app.select_first_if_needed();
        app
    }

    pub fn selected_index(&self) -> Option<usize> {
        let key = self.selected_session.as_ref()?;
        self.filtered_sessions
            .iter()
            .copied()
            .find(|idx| self.snapshot.sessions[*idx].key == *key)
    }

    pub fn selected_session(&self) -> Option<&crate::aggregate::SessionStats> {
        self.selected_index()
            .map(|idx| &self.snapshot.sessions[idx])
    }

    pub fn selected_filtered_position(&self) -> Option<usize> {
        let key = self.selected_session.as_ref()?;
        self.filtered_sessions
            .iter()
            .position(|idx| self.snapshot.sessions[*idx].key == *key)
    }

    pub fn move_down(&mut self) {
        self.move_selection(1);
    }

    pub fn move_up(&mut self) {
        self.move_selection(-1);
    }

    pub fn scroll_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn page_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(8);
    }

    pub fn page_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(8);
    }

    pub fn toggle_breakdown(&mut self) {
        self.breakdown = !self.breakdown;
        self.detail_scroll = 0;
    }

    pub fn start_search(&mut self) {
        self.search.mode = SearchMode::Editing;
    }

    pub fn cancel_search(&mut self) {
        self.search = SearchState::default();
        self.rebuild_filter();
        self.select_first_if_needed();
    }

    pub fn confirm_search(&mut self) {
        self.search.mode = if self.search.query.is_empty() {
            SearchMode::Inactive
        } else {
            SearchMode::Confirmed
        };
        self.rebuild_filter();
        self.select_first_if_needed();
    }

    pub fn push_search_char(&mut self, ch: char) {
        self.search.query.push(ch);
        self.rebuild_filter();
        self.clamp_selection();
    }

    pub fn pop_search_char(&mut self) {
        self.search.query.pop();
        self.rebuild_filter();
        self.clamp_selection();
    }

    pub fn replace_snapshot(&mut self, snapshot: AggregateSnapshot) {
        let previous = self.selected_session.clone();
        self.snapshot = snapshot;
        self.footer_error = None;
        self.rebuild_filter();
        if let Some(key) = previous.filter(|key| {
            self.filtered_sessions
                .iter()
                .any(|idx| self.snapshot.sessions[*idx].key == *key)
        }) {
            self.selected_session = Some(key);
        } else {
            self.selected_session = self
                .filtered_sessions
                .first()
                .map(|idx| self.snapshot.sessions[*idx].key.clone());
        }
        self.sync_list_state();
        self.detail_scroll = 0;
    }

    pub fn rebuild_filter(&mut self) {
        let query = self.search.query.trim().to_ascii_lowercase();
        self.filtered_sessions = self
            .snapshot
            .sessions
            .iter()
            .enumerate()
            .filter_map(|(idx, session)| {
                if query.is_empty() || matches_session(session, &query) {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();
        self.sync_list_state();
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered_sessions.is_empty() {
            self.selected_session = None;
            self.session_list_state.select(None);
            return;
        }
        let current = self.selected_filtered_position().unwrap_or(0) as isize;
        let last = self.filtered_sessions.len() as isize - 1;
        let next = (current + delta).clamp(0, last) as usize;
        let idx = self.filtered_sessions[next];
        let key = self.snapshot.sessions[idx].key.clone();
        if self.selected_session.as_ref() != Some(&key) {
            self.detail_scroll = 0;
        }
        self.selected_session = Some(key);
        self.session_list_state.select(Some(next));
    }

    fn select_first_if_needed(&mut self) {
        if self.selected_session.is_none() {
            self.selected_session = self
                .filtered_sessions
                .first()
                .map(|idx| self.snapshot.sessions[*idx].key.clone());
        }
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        if self.filtered_sessions.is_empty() {
            self.selected_session = None;
            self.session_list_state.select(None);
            self.detail_scroll = 0;
            return;
        }
        if self.selected_filtered_position().is_none() {
            self.selected_session = self
                .filtered_sessions
                .first()
                .map(|idx| self.snapshot.sessions[*idx].key.clone());
            self.detail_scroll = 0;
        }
        self.sync_list_state();
    }

    fn sync_list_state(&mut self) {
        self.session_list_state
            .select(self.selected_filtered_position());
    }
}

fn matches_session(session: &crate::aggregate::SessionStats, query: &str) -> bool {
    let id = session.key.id.0.to_ascii_lowercase();
    let qualified = session.key.qualified().to_ascii_lowercase();
    let name = session
        .name
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    id.contains(query) || qualified.contains(query) || name.contains(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        aggregate::SessionStats,
        domain::{Client, SessionId, SessionKey, TokenStats},
    };
    use chrono::Utc;

    fn snapshot(ids: &[(&str, u64)]) -> AggregateSnapshot {
        AggregateSnapshot {
            generated_at: Utc::now(),
            sessions: ids
                .iter()
                .map(|(id, total)| SessionStats {
                    key: SessionKey {
                        client: Client::Gjc,
                        id: SessionId((*id).into()),
                    },
                    parent: None,
                    name: Some((*id).into()),
                    started_at: None,
                    tokens: TokenStats {
                        total_tokens: *total,
                        ..Default::default()
                    },
                    models: vec![],
                    record_count: 0,
                })
                .collect(),
            diagnostics: vec![],
            source_counts: Default::default(),
        }
    }

    #[test]
    fn empty_session_list_is_safe() {
        let app = App::new(snapshot(&[]));
        assert!(app.selected_session.is_none());
        assert!(app.filtered_sessions.is_empty());
    }

    #[test]
    fn selection_stays_in_range_after_search() {
        let mut app = App::new(snapshot(&[("alpha", 3), ("beta", 2)]));
        app.move_down();
        app.start_search();
        app.push_search_char('a');
        app.push_search_char('l');
        assert_eq!(app.selected_session().unwrap().key.id.0, "alpha");
    }

    #[test]
    fn refresh_keeps_session_key() {
        let mut app = App::new(snapshot(&[("alpha", 3), ("beta", 2)]));
        app.move_down();
        app.replace_snapshot(snapshot(&[("beta", 10), ("alpha", 1)]));
        assert_eq!(app.selected_session().unwrap().key.id.0, "beta");
    }

    #[test]
    fn refresh_selects_closest_available_when_session_disappears() {
        let mut app = App::new(snapshot(&[("alpha", 3), ("beta", 2)]));
        app.move_down();
        app.replace_snapshot(snapshot(&[("gamma", 10)]));
        assert_eq!(app.selected_session().unwrap().key.id.0, "gamma");
    }

    #[test]
    fn changing_session_resets_detail_scroll() {
        let mut app = App::new(snapshot(&[("alpha", 3), ("beta", 2)]));
        app.scroll_down();
        app.move_down();
        assert_eq!(app.detail_scroll, 0);
    }
}
