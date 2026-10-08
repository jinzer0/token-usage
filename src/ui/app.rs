use crate::{aggregate::AggregateSnapshot, domain::SessionKey};
use chrono::NaiveDate;
use ratatui::widgets::ListState;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DetailScope {
    All,
    Day(NaiveDate),
    Undated,
}

impl DetailScope {
    pub fn label(self) -> String {
        match self {
            Self::All => "All".into(),
            Self::Day(day) => format!("{day} · Local"),
            Self::Undated => "Unknown date".into(),
        }
    }
}

pub struct DatePickerState {
    pub options: Vec<DetailScope>,
    pub cursor: usize,
}

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
    pub footer_notice: Option<String>,
    pub detail_scope: DetailScope,
    pub date_picker: Option<DatePickerState>,
    pub date_picker_available: bool,
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
            footer_notice: None,
            detail_scope: DetailScope::All,
            date_picker: None,
            date_picker_available: false,
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
        self.footer_notice = None;
        self.rebuild_filter();
        if let Some(key) = previous.clone().filter(|key| {
            self.filtered_sessions
                .iter()
                .any(|idx| self.snapshot.sessions[*idx].key == *key)
        }) {
            self.set_selected_key(Some(key));
        } else {
            let key = self
                .filtered_sessions
                .first()
                .map(|idx| self.snapshot.sessions[*idx].key.clone());
            self.set_selected_key(key);
        }
        self.sync_list_state();
        if previous != self.selected_session {
            self.footer_notice = Some(if self.selected_session.is_some() {
                "New session; All".into()
            } else {
                "Session lost; All".into()
            });
        } else if !self.scope_available(self.detail_scope) {
            self.detail_scope = DetailScope::All;
            self.date_picker = None;
            self.footer_notice = Some("Date gone; All".into());
        }
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
            self.set_selected_key(None);
            self.session_list_state.select(None);
            return;
        }
        let current = self.selected_filtered_position().unwrap_or(0) as isize;
        let last = self.filtered_sessions.len() as isize - 1;
        let next = (current + delta).clamp(0, last) as usize;
        let idx = self.filtered_sessions[next];
        let key = self.snapshot.sessions[idx].key.clone();
        self.set_selected_key(Some(key));
        self.session_list_state.select(Some(next));
    }

    fn select_first_if_needed(&mut self) {
        if self.selected_session.is_none() {
            let key = self
                .filtered_sessions
                .first()
                .map(|idx| self.snapshot.sessions[*idx].key.clone());
            self.set_selected_key(key);
        }
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        if self.filtered_sessions.is_empty() {
            self.set_selected_key(None);
            self.session_list_state.select(None);
            self.detail_scroll = 0;
            return;
        }
        if self.selected_filtered_position().is_none() {
            let key = self
                .filtered_sessions
                .first()
                .map(|idx| self.snapshot.sessions[*idx].key.clone());
            self.set_selected_key(key);
            self.detail_scroll = 0;
        }
        self.sync_list_state();
    }

    fn sync_list_state(&mut self) {
        self.session_list_state
            .select(self.selected_filtered_position());
    }

    fn set_selected_key(&mut self, key: Option<SessionKey>) {
        if self.selected_session != key {
            self.selected_session = key;
            self.detail_scope = DetailScope::All;
            self.date_picker = None;
            self.footer_notice = None;
            self.detail_scroll = 0;
        }
    }

    pub fn scope_available(&self, scope: DetailScope) -> bool {
        match scope {
            DetailScope::All => true,
            DetailScope::Day(day) => self.selected_session().is_some_and(|s| {
                s.daily
                    .iter()
                    .any(|d| d.day == Some(day) && d.record_count > 0)
            }),
            DetailScope::Undated => self.selected_session().is_some_and(|s| {
                s.daily
                    .iter()
                    .any(|d| d.day.is_none() && d.record_count > 0)
            }),
        }
    }

    pub fn update_picker_availability(&mut self, available: bool) {
        self.date_picker_available = available && self.selected_session().is_some();
        if !self.date_picker_available {
            self.date_picker = None;
        }
    }

    pub fn open_date_picker(&mut self) {
        if !self.date_picker_available {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let mut options = vec![DetailScope::All];
        options.extend(
            session
                .daily
                .iter()
                .filter(|d| d.record_count > 0)
                .map(|d| d.day.map(DetailScope::Day).unwrap_or(DetailScope::Undated)),
        );
        let cursor = options
            .iter()
            .position(|s| *s == self.detail_scope)
            .unwrap_or(0);
        self.date_picker = Some(DatePickerState { options, cursor });
    }

    pub fn move_date_cursor(&mut self, delta: isize) {
        if let Some(picker) = &mut self.date_picker {
            picker.cursor = picker
                .cursor
                .saturating_add_signed(delta)
                .min(picker.options.len().saturating_sub(1));
        }
    }

    pub fn apply_date_picker(&mut self) {
        let Some(picker) = self.date_picker.take() else {
            return;
        };
        if let Some(scope) = picker.options.get(picker.cursor).copied()
            && self.scope_available(scope)
        {
            if self.detail_scope != scope {
                self.detail_scroll = 0;
            }
            self.detail_scope = scope;
            self.footer_notice = None;
        }
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
pub(crate) fn daily_test_snapshot(keep: [bool; 3]) -> AggregateSnapshot {
    use crate::{
        aggregate::aggregate,
        domain::{Client, ReasoningEffort, SessionId, TokenStats, UsageRecord},
        scan::ParseResult,
    };
    let mut result = ParseResult::empty(Client::Gjc, "daily-fixture".into());
    for (index, (time, model, total, input, reasoning, unknown)) in [
        (Some("2026-03-10T12:00:00Z"), "day-old", 100, 60, 10, false),
        (Some("2026-03-11T12:00:00Z"), "day-new", 300, 200, 20, true),
        (None, "day-unknown", 700, 400, 0, true),
    ]
    .into_iter()
    .enumerate()
    {
        if !keep[index] {
            continue;
        }
        result.records.push(UsageRecord {
            session_key: SessionKey {
                client: Client::Gjc,
                id: SessionId("daily-session".into()),
            },
            parent_session_key: None,
            message_id: None,
            source_path: "daily-fixture".into(),
            source_line: Some(index as u64 + 1),
            started_at: time.map(|t| {
                chrono::DateTime::parse_from_rfc3339(t)
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            }),
            session_name: Some("Daily fixture".into()),
            model: model.into(),
            reasoning_effort: Some(ReasoningEffort::High),
            tokens: TokenStats {
                total_tokens: total,
                input_total: input,
                input_uncached: input - 10,
                cache_read: 10,
                cache_write: 0,
                output_total: total - input,
                reasoning_known: reasoning,
                reasoning_has_unknown: unknown,
            },
        });
    }
    aggregate(vec![result])
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
                    daily: vec![],
                })
                .collect(),
            timeline: vec![],
            periods: Default::default(),
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

    #[test]
    fn daily_picker_cancel_commit_and_same_key_navigation() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.update_picker_availability(true);
        app.detail_scroll = 7;
        app.open_date_picker();
        assert_eq!(app.date_picker.as_ref().unwrap().options.len(), 4);
        assert_eq!(
            app.date_picker.as_ref().unwrap().options.last(),
            Some(&DetailScope::Undated)
        );
        app.move_date_cursor(1);
        app.date_picker = None;
        assert_eq!(app.detail_scope, DetailScope::All);
        assert_eq!(app.detail_scroll, 7);
        app.open_date_picker();
        app.move_date_cursor(1);
        app.apply_date_picker();
        assert!(matches!(app.detail_scope, DetailScope::Day(_)));
        assert_eq!(app.detail_scroll, 0);
        let selected = app.detail_scope;
        app.move_down();
        assert_eq!(app.detail_scope, selected);
        app.start_search();
        app.push_search_char('d');
        app.cancel_search();
        assert_eq!(app.detail_scope, selected);
    }

    #[test]
    fn daily_key_changes_search_empty_and_client_collision_reset_scope() {
        let mut snapshot = daily_test_snapshot([true; 3]);
        let mut other = snapshot.sessions[0].clone();
        other.key.client = Client::Codex;
        snapshot.sessions.push(other);
        let mut app = App::new(snapshot);
        app.detail_scope = DetailScope::Undated;
        app.move_down();
        assert_eq!(app.selected_session().unwrap().key.client, Client::Codex);
        assert_eq!(app.detail_scope, DetailScope::All);
        app.detail_scope = DetailScope::Undated;
        app.start_search();
        for ch in "gjc:".chars() {
            app.push_search_char(ch);
        }
        assert_eq!(app.selected_session().unwrap().key.client, Client::Gjc);
        assert_eq!(app.detail_scope, DetailScope::All);
        app.detail_scope = DetailScope::Undated;
        app.push_search_char('x');
        assert!(app.selected_session().is_none());
        assert_eq!(app.detail_scope, DetailScope::All);
    }

    #[test]
    fn daily_refresh_preserves_valid_and_explains_invalid_date_or_undated() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        let day = app.selected_session().unwrap().daily[0].day.unwrap();
        app.detail_scope = DetailScope::Day(day);
        app.replace_snapshot(daily_test_snapshot([true; 3]));
        assert_eq!(app.detail_scope, DetailScope::Day(day));
        assert!(app.footer_notice.is_none());
        app.replace_snapshot(daily_test_snapshot([true, false, true]));
        assert_eq!(app.detail_scope, DetailScope::All);
        assert!(app.footer_notice.as_ref().unwrap().contains("Date gone"));
        app.detail_scope = DetailScope::Undated;
        app.replace_snapshot(daily_test_snapshot([true, false, true]));
        assert_eq!(app.detail_scope, DetailScope::Undated);
        app.replace_snapshot(daily_test_snapshot([true, true, false]));
        assert_eq!(app.detail_scope, DetailScope::All);
        assert!(app.footer_notice.is_some());
        app.replace_snapshot(daily_test_snapshot([false; 3]));
        assert!(app.selected_session.is_none());
        assert_eq!(app.detail_scope, DetailScope::All);
    }

    #[test]
    fn daily_refresh_reorder_new_date_and_zero_token_scope_stay_keyed() {
        use crate::aggregate::{EffortStats, ModelStats, SessionDayStats};
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.update_picker_availability(true);
        let day = app.selected_session().unwrap().daily[0].day.unwrap();
        app.detail_scope = DetailScope::Day(day);
        let mut refreshed = daily_test_snapshot([true; 3]);
        let session = &mut refreshed.sessions[0];
        let zero_model = ModelStats {
            model: "zero-new".into(),
            tokens: TokenStats::default(),
            record_count: 1,
            efforts: vec![EffortStats {
                effort: None,
                tokens: TokenStats::default(),
                record_count: 1,
            }],
        };
        let new_day = day + chrono::Duration::days(1);
        session.daily.insert(
            0,
            SessionDayStats {
                day: Some(new_day),
                tokens: TokenStats::default(),
                models: vec![zero_model.clone()],
                record_count: 1,
            },
        );
        session.models.push(zero_model);
        session.record_count += 1;
        let mut other = session.clone();
        other.key.client = Client::Codex;
        refreshed.sessions.insert(0, other);
        app.replace_snapshot(refreshed.clone());
        assert_eq!(app.selected_session().unwrap().key.client, Client::Gjc);
        assert_eq!(app.selected_index(), Some(1));
        assert_eq!(app.detail_scope, DetailScope::Day(day));
        app.open_date_picker();
        assert_eq!(app.date_picker.as_ref().unwrap().cursor, 2);
        app.move_date_cursor(-1);
        app.apply_date_picker();
        assert_eq!(app.detail_scope, DetailScope::Day(new_day));
        app.replace_snapshot(refreshed);
        assert_eq!(app.detail_scope, DetailScope::Day(new_day));
        assert!(app.footer_notice.is_none());
    }
}
