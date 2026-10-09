use crate::{
    aggregate::{AggregateSnapshot, DateStats, SessionDayStats, SessionStats},
    domain::{SessionKey, TokenStats},
};
use chrono::NaiveDate;
use ratatui::widgets::ListState;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ViewTab {
    Sessions,
    Dates,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SessionSort {
    LastUsed,
    Tokens,
}
impl SessionSort {
    pub fn label(self) -> &'static str {
        match self {
            Self::LastUsed => "Last used",
            Self::Tokens => "Tokens",
        }
    }
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PaneFocus {
    Dates,
    Sessions,
    Details,
}
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

#[derive(Clone)]
struct TabState {
    selected_session: Option<SessionKey>,
    sort: SessionSort,
    focus: PaneFocus,
    list: ListState,
    detail_scroll: usize,
}
impl TabState {
    fn new(focus: PaneFocus) -> Self {
        Self {
            selected_session: None,
            sort: SessionSort::LastUsed,
            focus,
            list: ListState::default(),
            detail_scroll: 0,
        }
    }
}

pub struct App {
    pub snapshot: AggregateSnapshot,
    pub tab: ViewTab,
    pub sort: SessionSort,
    pub focus: PaneFocus,
    pub pinned_session: Option<SessionKey>,
    pub date_list_state: ListState,
    pub visible_dates: Vec<usize>,
    pub selected_date: Option<DetailScope>,
    pub session_list_state: ListState,
    pub selected_session: Option<SessionKey>,
    pub filtered_sessions: Vec<usize>,
    pub detail_scope: DetailScope,
    pub detail_scroll: usize,
    pub breakdown: bool,
    pub search: SearchState,
    pub help_visible: bool,
    pub footer_error: Option<String>,
    pub footer_notice: Option<String>,
    sessions_state: TabState,
    dates_state: TabState,
}
impl App {
    pub fn new(snapshot: AggregateSnapshot) -> Self {
        let mut app = Self {
            snapshot,
            tab: ViewTab::Sessions,
            sort: SessionSort::LastUsed,
            focus: PaneFocus::Sessions,
            pinned_session: None,
            date_list_state: ListState::default(),
            visible_dates: Vec::new(),
            selected_date: None,
            session_list_state: ListState::default(),
            selected_session: None,
            filtered_sessions: Vec::new(),
            detail_scope: DetailScope::All,
            detail_scroll: 0,
            breakdown: false,
            search: SearchState::default(),
            help_visible: false,
            footer_error: None,
            footer_notice: None,
            sessions_state: TabState::new(PaneFocus::Sessions),
            dates_state: TabState::new(PaneFocus::Dates),
        };
        app.rebuild_filter();
        app
    }
    pub fn selected_index(&self) -> Option<usize> {
        let key = self.selected_session.as_ref()?;
        self.filtered_sessions
            .iter()
            .copied()
            .find(|&i| self.snapshot.sessions[i].key == *key)
    }
    pub fn selected_session(&self) -> Option<&SessionStats> {
        self.selected_index().map(|i| &self.snapshot.sessions[i])
    }
    pub fn selected_filtered_position(&self) -> Option<usize> {
        let key = self.selected_session.as_ref()?;
        self.filtered_sessions
            .iter()
            .position(|&i| self.snapshot.sessions[i].key == *key)
    }
    pub fn selected_date_position(&self) -> Option<usize> {
        let scope = self.selected_date?;
        self.visible_dates
            .iter()
            .position(|&i| date_scope(self.snapshot.dates[i].day) == scope)
    }
    pub fn selected_date_stats(&self) -> Option<&DateStats> {
        self.selected_date_position()
            .map(|p| &self.snapshot.dates[self.visible_dates[p]])
    }
    pub fn selected_day_stats(&self, session_index: usize) -> Option<&SessionDayStats> {
        let scope = self.selected_date?;
        self.snapshot
            .sessions
            .get(session_index)?
            .daily
            .iter()
            .find(|d| d.record_count > 0 && date_scope(d.day) == scope)
    }
    pub fn selected_tokens(&self, session_index: usize) -> Option<&TokenStats> {
        match self.tab {
            ViewTab::Sessions => self.snapshot.sessions.get(session_index).map(|s| &s.tokens),
            ViewTab::Dates => self.selected_day_stats(session_index).map(|d| &d.tokens),
        }
    }
    pub fn date_tokens(&self, date_index: usize) -> Option<&TokenStats> {
        let date = self.snapshot.dates.get(date_index)?;
        match &self.pinned_session {
            None => Some(&date.tokens),
            Some(key) => self
                .snapshot
                .sessions
                .iter()
                .find(|s| s.key == *key)?
                .daily
                .iter()
                .find(|d| d.day == date.day && d.record_count > 0)
                .map(|d| &d.tokens),
        }
    }
    pub fn scope_available(&self, scope: DetailScope) -> bool {
        scope == DetailScope::All
            || self.selected_session().is_some_and(|s| {
                s.daily
                    .iter()
                    .any(|d| d.record_count > 0 && date_scope(d.day) == scope)
            })
    }
    pub fn move_down(&mut self) {
        self.move_active(1);
    }
    pub fn move_up(&mut self) {
        self.move_active(-1);
    }
    fn move_active(&mut self, delta: isize) {
        match self.focus {
            PaneFocus::Dates if self.tab == ViewTab::Dates => self.move_date(delta),
            PaneFocus::Details => {
                self.detail_scroll = self.detail_scroll.saturating_add_signed(delta)
            }
            _ => self.move_selection(delta),
        }
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
    pub fn move_focus(&mut self, delta: isize) {
        let panes: &[PaneFocus] = match self.tab {
            ViewTab::Sessions => &[PaneFocus::Sessions, PaneFocus::Details],
            ViewTab::Dates => &[PaneFocus::Dates, PaneFocus::Sessions, PaneFocus::Details],
        };
        let current = panes.iter().position(|p| *p == self.focus).unwrap_or(0);
        self.focus = panes[(current as isize + delta).rem_euclid(panes.len() as isize) as usize];
    }
    pub fn switch_tab(&mut self, tab: ViewTab) {
        if self.tab == tab {
            return;
        }
        self.save_tab();
        let state = match tab {
            ViewTab::Sessions => &self.sessions_state,
            ViewTab::Dates => &self.dates_state,
        }
        .clone();
        self.tab = tab;
        self.selected_session = state.selected_session;
        self.sort = state.sort;
        self.focus = state.focus;
        self.session_list_state = state.list;
        self.detail_scroll = state.detail_scroll;
        self.rebuild_filter();
    }
    fn save_tab(&mut self) {
        let state = TabState {
            selected_session: self.selected_session.clone(),
            sort: self.sort,
            focus: self.focus,
            list: self.session_list_state.clone(),
            detail_scroll: self.detail_scroll,
        };
        match self.tab {
            ViewTab::Sessions => self.sessions_state = state,
            ViewTab::Dates => self.dates_state = state,
        }
    }
    pub fn toggle_sort(&mut self) {
        self.sort = match self.sort {
            SessionSort::LastUsed => SessionSort::Tokens,
            SessionSort::Tokens => SessionSort::LastUsed,
        };
        self.rebuild_filter();
    }
    pub fn open_session_dates(&mut self) {
        let Some(key) = self.selected_session.clone() else {
            return;
        };
        let latest = self
            .selected_session()
            .and_then(|s| {
                s.daily
                    .iter()
                    .filter(|d| d.record_count > 0)
                    .max_by_key(|d| d.day)
            })
            .map(|d| date_scope(d.day));
        self.switch_tab(ViewTab::Dates);
        self.pinned_session = Some(key.clone());
        self.selected_date = latest;
        self.selected_session = Some(key);
        self.focus = PaneFocus::Dates;
        self.detail_scroll = 0;
        self.rebuild_filter();
    }
    pub fn toggle_pin(&mut self) {
        if self.tab != ViewTab::Dates {
            return;
        }
        self.pinned_session = if self.pinned_session.is_some() {
            None
        } else {
            self.selected_session.clone()
        };
        self.rebuild_filter();
    }
    pub fn move_date(&mut self, delta: isize) {
        if self.tab != ViewTab::Dates || self.visible_dates.is_empty() {
            return;
        }
        let p = self
            .selected_date_position()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(self.visible_dates.len() - 1);
        let scope = date_scope(self.snapshot.dates[self.visible_dates[p]].day);
        if self.selected_date != Some(scope) {
            self.selected_date = Some(scope);
            self.detail_scroll = 0;
            if let Some(key) = &self.pinned_session {
                self.selected_session = Some(key.clone());
            }
            self.rebuild_filter();
        }
    }
    fn move_selection(&mut self, delta: isize) {
        if self.filtered_sessions.is_empty() {
            return;
        }
        let p = self
            .selected_filtered_position()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(self.filtered_sessions.len() - 1);
        let key = self.snapshot.sessions[self.filtered_sessions[p]]
            .key
            .clone();
        if self.selected_session.as_ref() != Some(&key) {
            self.selected_session = Some(key.clone());
            self.detail_scroll = 0;
            if self.tab == ViewTab::Dates
                && self.pinned_session.as_ref().is_some_and(|pin| *pin != key)
            {
                self.pinned_session = None;
                self.rebuild_filter();
            }
        }
        self.session_list_state
            .select(self.selected_filtered_position());
    }
    pub fn start_search(&mut self) {
        self.search.mode = SearchMode::Editing;
    }
    pub fn cancel_search(&mut self) {
        self.search = SearchState::default();
        self.rebuild_filter();
    }
    pub fn confirm_search(&mut self) {
        self.search.mode = if self.search.query.is_empty() {
            SearchMode::Inactive
        } else {
            SearchMode::Confirmed
        };
        self.rebuild_filter();
    }
    pub fn push_search_char(&mut self, ch: char) {
        self.search.query.push(ch);
        self.rebuild_filter();
    }
    pub fn pop_search_char(&mut self) {
        self.search.query.pop();
        self.rebuild_filter();
    }
    pub fn rebuild_filter(&mut self) {
        let query = self.search.query.trim().to_ascii_lowercase();
        if self.pinned_session.as_ref().is_some_and(|key| {
            !self
                .snapshot
                .sessions
                .iter()
                .any(|s| s.key == *key && matches_session(s, &query))
        }) {
            self.pinned_session = None;
            self.footer_notice = Some("Pinned session unavailable; unpinned".into());
        }
        self.visible_dates = self
            .snapshot
            .dates
            .iter()
            .enumerate()
            .filter(|(_, d)| d.record_count > 0)
            .filter(|(_, d)| {
                self.pinned_session.as_ref().is_none_or(|key| {
                    self.snapshot.sessions.iter().any(|s| {
                        s.key == *key
                            && s.daily
                                .iter()
                                .any(|sd| sd.day == d.day && sd.record_count > 0)
                    })
                })
            })
            .map(|(i, _)| i)
            .collect();
        self.visible_dates
            .sort_by(|&a, &b| self.snapshot.dates[b].day.cmp(&self.snapshot.dates[a].day));
        if self.selected_date_position().is_none() {
            self.selected_date = self
                .visible_dates
                .first()
                .map(|&i| date_scope(self.snapshot.dates[i].day));
        }
        self.date_list_state.select(self.selected_date_position());
        self.filtered_sessions = self
            .snapshot
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| matches_session(s, &query))
            .filter(|(i, _)| self.tab == ViewTab::Sessions || self.selected_day_stats(*i).is_some())
            .map(|(i, _)| i)
            .collect();
        let mut indices = std::mem::take(&mut self.filtered_sessions);
        indices.sort_by(|&a, &b| {
            let sa = &self.snapshot.sessions[a];
            let sb = &self.snapshot.sessions[b];
            let primary = match self.sort {
                SessionSort::LastUsed => sb.last_used_at.cmp(&sa.last_used_at),
                SessionSort::Tokens => self
                    .selected_tokens(b)
                    .map(|t| t.total_tokens)
                    .cmp(&self.selected_tokens(a).map(|t| t.total_tokens)),
            };
            primary
                .then_with(|| sa.key.id.cmp(&sb.key.id))
                .then_with(|| sa.key.client.cmp(&sb.key.client))
        });
        self.filtered_sessions = indices;
        if self.selected_filtered_position().is_none() {
            let key = self
                .filtered_sessions
                .first()
                .map(|&i| self.snapshot.sessions[i].key.clone());
            if self.selected_session != key {
                self.detail_scroll = 0;
            }
            self.selected_session = key;
        }
        self.session_list_state
            .select(self.selected_filtered_position());
        self.detail_scope = match self.tab {
            ViewTab::Sessions => DetailScope::All,
            ViewTab::Dates => self.selected_date.unwrap_or(DetailScope::All),
        };
    }
    pub fn replace_snapshot(&mut self, snapshot: AggregateSnapshot) {
        self.save_tab();
        let old_date = self.selected_date;
        let old_date_key = self.dates_state.selected_session.clone();
        let old_pin = self.pinned_session.clone();
        let old_key = self.selected_session.clone();
        self.snapshot = snapshot;
        self.footer_error = None;
        self.footer_notice = None;
        let mut notices = Vec::new();
        if old_pin
            .as_ref()
            .is_some_and(|k| !self.snapshot.sessions.iter().any(|s| s.key == *k))
        {
            self.pinned_session = None;
            notices.push("Pinned session gone; unpinned");
        }
        if old_date.is_some_and(|scope| {
            !self
                .snapshot
                .dates
                .iter()
                .any(|d| date_scope(d.day) == scope && d.record_count > 0)
        }) {
            notices.push("Date gone; latest available date");
        }
        if [
            &self.sessions_state.selected_session,
            &self.dates_state.selected_session,
        ]
        .into_iter()
        .any(|key| {
            key.as_ref()
                .is_some_and(|k| !self.snapshot.sessions.iter().any(|s| s.key == *k))
        }) {
            notices.push("Session gone; first available session");
        }
        self.rebuild_filter();
        if old_date != self.selected_date {
            self.dates_state.detail_scroll = 0;
            if old_date.is_some() && !notices.contains(&"Date gone; latest available date") {
                notices.push("Date gone; latest available date");
            }
        }
        if (self.tab == ViewTab::Dates && old_date != self.selected_date)
            || old_key != self.selected_session
        {
            self.detail_scroll = 0;
        }
        // Reconcile the inactive view as well, so stale selections and scroll never reappear.
        let active = self.tab;
        self.switch_tab(match active {
            ViewTab::Sessions => ViewTab::Dates,
            ViewTab::Dates => ViewTab::Sessions,
        });
        self.switch_tab(active);
        if old_date_key.is_some()
            && old_date_key != self.dates_state.selected_session
            && old_date == self.selected_date
            && !notices.contains(&"Session gone; first available session")
        {
            notices.push("Session date gone; first available session");
        }
        if !notices.is_empty() {
            self.footer_notice = Some(notices.join("; "));
        }
    }
}
fn date_scope(day: Option<NaiveDate>) -> DetailScope {
    day.map(DetailScope::Day).unwrap_or(DetailScope::Undated)
}
fn matches_session(session: &SessionStats, query: &str) -> bool {
    query.is_empty()
        || session.key.id.0.to_ascii_lowercase().contains(query)
        || session.key.qualified().to_ascii_lowercase().contains(query)
        || session
            .name
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains(query)
}

#[cfg(test)]
pub(crate) fn daily_test_snapshot(keep: [bool; 3]) -> AggregateSnapshot {
    use crate::{
        aggregate::aggregate,
        domain::{Client, ReasoningEffort, SessionId, UsageRecord},
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
            started_at: time.map(|time| time.parse().unwrap()),
            session_name: None,
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
    aggregate(vec![result]).unwrap()
}
#[cfg(test)]
fn test_snapshot(
    rows: &[(&str, crate::domain::Client, Option<&str>, u64, bool)],
) -> AggregateSnapshot {
    use crate::{
        aggregate::aggregate,
        domain::{Client, SessionId, UsageRecord},
        scan::ParseResult,
    };
    let mut result = ParseResult::empty(Client::Gjc, "ui-fixture".into());
    for (id, client, time, total, keep) in rows {
        if !*keep {
            continue;
        }
        result.records.push(UsageRecord {
            session_key: SessionKey {
                client: *client,
                id: SessionId((*id).into()),
            },
            parent_session_key: None,
            message_id: None,
            source_path: "ui-fixture".into(),
            source_line: None,
            started_at: time.map(|t| {
                chrono::DateTime::parse_from_rfc3339(t)
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            }),
            session_name: None,
            model: "model".into(),
            reasoning_effort: None,
            tokens: TokenStats {
                total_tokens: *total,
                reasoning_has_unknown: true,
                ..TokenStats::default()
            },
        });
    }
    aggregate(vec![result]).unwrap()
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Client::{Codex, Gjc};
    fn ids(app: &App) -> Vec<&str> {
        app.filtered_sessions
            .iter()
            .map(|&i| app.snapshot.sessions[i].key.id.0.as_str())
            .collect()
    }
    #[test]
    fn precise_last_used_unknown_and_id_client_ties_do_not_change_cli_order() {
        let snapshot = test_snapshot(&[
            ("late", Gjc, Some("2026-03-11T12:00:00.002Z"), 0, true),
            ("early", Gjc, Some("2026-03-11T12:00:00.001Z"), 10, true),
            ("same", Codex, None, 90, true),
            ("same", Gjc, None, 90, true),
        ]);
        let stored = snapshot
            .sessions
            .iter()
            .map(|s| s.key.clone())
            .collect::<Vec<_>>();
        let app = App::new(snapshot);
        assert_eq!(ids(&app), ["late", "early", "same", "same"]);
        assert!(
            app.snapshot.sessions[app.filtered_sessions[2]].key.client
                < app.snapshot.sessions[app.filtered_sessions[3]].key.client
        );
        assert_eq!(
            app.snapshot
                .sessions
                .iter()
                .map(|s| s.key.clone())
                .collect::<Vec<_>>(),
            stored
        );
    }
    #[test]
    fn dates_use_day_tokens_but_global_last_used_and_keep_zero_unknown() {
        let mut app = App::new(test_snapshot(&[
            ("a", Gjc, Some("2026-03-10T12:00:00Z"), 1, true),
            ("a", Gjc, Some("2026-03-11T12:00:00Z"), 1000, true),
            ("b", Gjc, Some("2026-03-10T12:00:00Z"), 9, true),
            ("zero", Gjc, None, 0, true),
        ]));
        app.switch_tab(ViewTab::Dates);
        app.move_date(1);
        assert_eq!(ids(&app), ["a", "b"]);
        app.toggle_sort();
        assert_eq!(ids(&app), ["b", "a"]);
        assert_eq!(
            app.selected_tokens(app.filtered_sessions[0])
                .unwrap()
                .total_tokens,
            9
        );
        app.move_date(1);
        assert_eq!(app.detail_scope, DetailScope::Undated);
        assert_eq!(ids(&app), ["zero"]);
        assert_eq!(
            app.selected_day_stats(app.filtered_sessions[0])
                .unwrap()
                .record_count,
            1
        );
        assert!(app.selected_tokens(usize::MAX).is_none());
        assert!(app.date_tokens(usize::MAX).is_none());
    }
    #[test]
    fn pin_limits_dates_not_middle_and_manual_selection_unpins() {
        let mut app = App::new(test_snapshot(&[
            ("a", Gjc, Some("2026-03-10T12:00:00Z"), 1, true),
            ("a", Gjc, Some("2026-03-11T12:00:00Z"), 3, true),
            ("b", Gjc, Some("2026-03-10T12:00:00Z"), 9, true),
            ("b", Gjc, None, 7, true),
        ]));
        app.open_session_dates();
        assert_eq!(app.visible_dates.len(), 2);
        app.move_date(1);
        assert_eq!(app.selected_session().unwrap().key.id.0, "a");
        assert_eq!(ids(&app), ["a", "b"]);
        let i = app.visible_dates[app.selected_date_position().unwrap()];
        assert_eq!(app.date_tokens(i).unwrap().total_tokens, 1);
        assert_eq!(app.snapshot.dates[i].tokens.total_tokens, 10);
        app.focus = PaneFocus::Sessions;
        app.move_down();
        assert!(app.pinned_session.is_none());
        assert_eq!(app.visible_dates.len(), 3);
        assert_eq!(app.selected_session().unwrap().key.id.0, "b");
    }
    #[test]
    fn search_keeps_global_dates_and_unpins_excluded_session() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.open_session_dates();
        let totals = app
            .snapshot
            .dates
            .iter()
            .map(|d| d.tokens.total_tokens)
            .collect::<Vec<_>>();
        app.start_search();
        app.push_search_char('x');
        assert!(app.pinned_session.is_none());
        assert!(app.selected_session.is_none());
        assert_eq!(app.visible_dates.len(), 3);
        assert_eq!(
            app.snapshot
                .dates
                .iter()
                .map(|d| d.tokens.total_tokens)
                .collect::<Vec<_>>(),
            totals
        );
        assert!(app.footer_notice.as_ref().unwrap().contains("unpinned"));
        app.cancel_search();
        assert!(app.selected_session.is_some());
    }
    #[test]
    fn tab_sort_selection_and_scroll_are_independent() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.toggle_sort();
        app.detail_scroll = 7;
        app.switch_tab(ViewTab::Dates);
        assert_eq!(app.sort, SessionSort::LastUsed);
        app.move_date(1);
        let date = app.selected_date;
        app.detail_scroll = 3;
        app.switch_tab(ViewTab::Sessions);
        assert_eq!(app.sort, SessionSort::Tokens);
        assert_eq!(app.detail_scope, DetailScope::All);
        assert_eq!(app.detail_scroll, 7);
        app.switch_tab(ViewTab::Dates);
        assert_eq!(app.selected_date, date);
        assert_eq!(app.detail_scroll, 3);
    }
    #[test]
    fn refresh_retains_identity_and_reports_date_pin_and_session_loss() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.open_session_dates();
        app.move_date(1);
        let date = app.selected_date;
        let key = app.selected_session.clone();
        app.replace_snapshot(daily_test_snapshot([true; 3]));
        assert_eq!(app.selected_date, date);
        assert_eq!(app.selected_session, key);
        assert!(app.footer_notice.is_none());
        app.replace_snapshot(daily_test_snapshot([false, true, true]));
        assert_ne!(app.selected_date, date);
        assert!(app.footer_notice.as_ref().unwrap().contains("Date gone"));
        app.replace_snapshot(daily_test_snapshot([false; 3]));
        assert!(
            app.selected_session.is_none()
                && app.selected_date.is_none()
                && app.pinned_session.is_none()
        );
        assert!(
            app.footer_notice
                .as_ref()
                .unwrap()
                .contains("Pinned session gone")
        );
        app.move_up();
        app.move_date(1);
        app.toggle_pin();
        assert!(app.filtered_sessions.is_empty());
    }
    #[test]
    fn refresh_reorders_indices_without_losing_day_or_qualified_identity() {
        let mut app = App::new(daily_test_snapshot([true; 3]));
        app.open_session_dates();
        app.move_date(1);
        let day = app.selected_date;
        let key = app.selected_session.clone();
        let replacement = test_snapshot(&[
            (
                "daily-session",
                Gjc,
                Some("2026-03-10T12:00:00Z"),
                100,
                true,
            ),
            (
                "daily-session",
                Gjc,
                Some("2026-03-11T12:00:00Z"),
                300,
                true,
            ),
            (
                "daily-session",
                Codex,
                Some("2026-03-10T12:00:00Z"),
                999,
                true,
            ),
            ("new", Gjc, Some("2026-03-12T12:00:00Z"), 0, true),
        ]);
        app.replace_snapshot(replacement);
        assert_eq!(app.selected_date, day);
        assert_eq!(app.selected_session, key);
        assert_eq!(app.selected_session().unwrap().key.client, Gjc);
        assert_eq!(
            app.selected_day_stats(app.selected_index().unwrap())
                .unwrap()
                .tokens
                .total_tokens,
            100
        );
    }

    #[test]
    fn refresh_notices_selected_day_loss_even_when_session_and_date_still_exist() {
        for active in [ViewTab::Dates, ViewTab::Sessions] {
            let mut app = App::new(test_snapshot(&[
                ("a", Gjc, Some("2026-03-10T12:00:00Z"), 9, true),
                ("a", Gjc, Some("2026-03-11T12:00:00Z"), 3, true),
                ("b", Gjc, Some("2026-03-11T12:00:00Z"), 2, true),
            ]));
            app.switch_tab(ViewTab::Dates);
            assert_eq!(app.selected_session().unwrap().key.id.0, "a");
            let day = app.selected_date;
            app.switch_tab(active);
            app.replace_snapshot(test_snapshot(&[
                ("a", Gjc, Some("2026-03-10T12:00:00Z"), 9, true),
                ("b", Gjc, Some("2026-03-11T12:00:00Z"), 2, true),
            ]));
            assert_eq!(app.selected_date, day);
            assert!(
                app.footer_notice
                    .as_deref()
                    .unwrap()
                    .contains("Session date gone")
            );
            app.switch_tab(ViewTab::Dates);
            assert_eq!(app.selected_session().unwrap().key.id.0, "b");
        }
    }

    #[test]
    fn date_comparison_keeps_full_unsigned_totals_and_unknown_last() {
        let mut app = App::new(test_snapshot(&[
            ("same", Gjc, Some("2026-03-10T12:00:00Z"), 1, true),
            (
                "same",
                Gjc,
                Some("2026-03-12T12:00:00Z"),
                u64::MAX - 1,
                true,
            ),
            ("same", Gjc, None, 0, true),
        ]));
        app.switch_tab(ViewTab::Dates);
        let totals = app
            .visible_dates
            .iter()
            .map(|&i| app.date_tokens(i).unwrap().total_tokens)
            .collect::<Vec<_>>();
        assert_eq!(totals, [u64::MAX - 1, 1, 0]);
        assert_eq!(
            i128::from(totals[0]) - i128::from(totals[1]),
            i128::from(u64::MAX) - 2
        );
        assert_eq!(
            app.snapshot.dates[*app.visible_dates.last().unwrap()].day,
            None
        );
        assert_eq!(app.snapshot.totals.total_tokens, u64::MAX);
    }

    #[test]
    fn qualified_search_confirm_cancel_and_refresh_session_fallback() {
        let mut app = App::new(test_snapshot(&[
            ("same", Gjc, None, 2, true),
            ("same", Codex, None, 1, true),
        ]));
        app.start_search();
        for ch in "gjc:same".chars() {
            app.push_search_char(ch);
        }
        app.confirm_search();
        assert_eq!(app.search.mode, SearchMode::Confirmed);
        assert_eq!(app.filtered_sessions.len(), 1);
        assert_eq!(app.selected_session().unwrap().key.client, Gjc);
        app.cancel_search();
        assert_eq!(app.filtered_sessions.len(), 2);
        app.detail_scroll = 9;
        app.replace_snapshot(test_snapshot(&[("replacement", Gjc, None, 0, true)]));
        assert_eq!(app.selected_session().unwrap().key.id.0, "replacement");
        assert_eq!(app.detail_scroll, 0);
        assert!(app.footer_notice.as_ref().unwrap().contains("Session gone"));
    }
}
