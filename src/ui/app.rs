use crate::{
    aggregate::{
        AggregateSnapshot, DateStats, EffortStats, ModelStats, SessionDayStats, SessionStats,
    },
    domain::{Client, ReasoningEffort, SessionKey, TokenStats},
};
use chrono::{Duration, Local, NaiveDate};
use ratatui::widgets::ListState;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ViewTab {
    Sessions,
    Dates,
    Models,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClientScope {
    All,
    Codex,
    Gjc,
    OpenCode,
}
impl ClientScope {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Codex,
            Self::Codex => Self::Gjc,
            Self::Gjc => Self::OpenCode,
            Self::OpenCode => Self::All,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Codex => "Codex",
            Self::Gjc => "GJC",
            Self::OpenCode => "OpenCode",
        }
    }
    pub fn matches(self, client: Client) -> bool {
        match self {
            Self::All => true,
            Self::Codex => client == Client::Codex,
            Self::Gjc => client == Client::Gjc,
            Self::OpenCode => client == Client::OpenCode,
        }
    }
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PeriodFilter {
    All,
    Today,
    SevenDays,
    ThirtyDays,
    CustomDays(u32),
}
impl PeriodFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Today,
            Self::Today => Self::SevenDays,
            Self::SevenDays => Self::ThirtyDays,
            Self::ThirtyDays | Self::CustomDays(_) => Self::All,
        }
    }
    pub fn label(self) -> Cow<'static, str> {
        match self {
            Self::All => Cow::Borrowed("All"),
            Self::Today => Cow::Borrowed("Today"),
            Self::SevenDays => Cow::Borrowed("7d"),
            Self::ThirtyDays => Cow::Borrowed("30d"),
            Self::CustomDays(n) => Cow::Owned(format!("{n}d")),
        }
    }
    pub fn matches_day(self, day: Option<NaiveDate>, today: NaiveDate) -> bool {
        match self {
            Self::All => true,
            Self::Today => day == Some(today),
            Self::SevenDays => day.is_some_and(|d| d >= today - Duration::days(6) && d <= today),
            Self::ThirtyDays => day.is_some_and(|d| d >= today - Duration::days(29) && d <= today),
            Self::CustomDays(n) => {
                n >= 1
                    && day.is_some_and(|d| {
                        d >= today - Duration::days(i64::from(n.saturating_sub(1))) && d <= today
                    })
            }
        }
    }
}
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct PeriodInputState {
    pub editing: bool,
    pub query: String,
}
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct SessionPeriodTotals {
    pub today: u64,
    pub seven_days: u64,
    pub thirty_days: u64,
    pub custom: Option<(u32, u64)>,
    pub all: u64,
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
pub enum ModelSort {
    Tokens,
    Sessions,
    Name,
}
impl ModelSort {
    pub fn next(self) -> Self {
        match self {
            Self::Tokens => Self::Sessions,
            Self::Sessions => Self::Name,
            Self::Name => Self::Tokens,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Tokens => "Tokens",
            Self::Sessions => "Sessions",
            Self::Name => "Name",
        }
    }
}
#[derive(Debug, Clone)]
pub struct ModelAggregateRow {
    pub model: String,
    pub tokens: TokenStats,
    pub record_count: u64,
    pub session_count: usize,
    pub clients: Vec<Client>,
    pub efforts: Vec<EffortStats>,
    pub top_sessions: Vec<(SessionKey, Option<String>, u64)>,
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DailyTrendPoint {
    pub day: NaiveDate,
    pub tokens: u64,
    pub record_count: u64,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PaneFocus {
    Dates,
    Sessions,
    Models,
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
    pub client_filter: ClientScope,
    pub period_filter: PeriodFilter,
    pub pinned_session: Option<SessionKey>,
    pub date_list_state: ListState,
    pub visible_dates: Vec<usize>,
    pub selected_date: Option<DetailScope>,
    pub session_list_state: ListState,
    pub selected_session: Option<SessionKey>,
    pub filtered_sessions: Vec<usize>,
    pub model_rows: Vec<ModelAggregateRow>,
    pub model_list_state: ListState,
    pub selected_model: Option<String>,
    pub trend_points: Vec<DailyTrendPoint>,
    pub model_sort: ModelSort,
    pub detail_scope: DetailScope,
    pub detail_scroll: usize,
    pub detail_max_scroll: usize,
    pub detail_total_lines: usize,
    pub detail_viewport_height: usize,
    pub breakdown: bool,
    pub search: SearchState,
    pub period_input: PeriodInputState,
    pub help_visible: bool,
    pub footer_error: Option<String>,
    pub footer_notice: Option<String>,
    sessions_state: TabState,
    dates_state: TabState,
    models_state: TabState,
}
impl App {
    pub fn new(snapshot: AggregateSnapshot) -> Self {
        Self::with_initial_session(snapshot, None)
    }
    pub fn with_initial_session(snapshot: AggregateSnapshot, initial: Option<SessionKey>) -> Self {
        let mut app = Self {
            snapshot,
            tab: ViewTab::Sessions,
            sort: SessionSort::LastUsed,
            focus: PaneFocus::Sessions,
            client_filter: ClientScope::All,
            period_filter: PeriodFilter::All,
            pinned_session: None,
            date_list_state: ListState::default(),
            visible_dates: Vec::new(),
            selected_date: None,
            session_list_state: ListState::default(),
            selected_session: initial,
            filtered_sessions: Vec::new(),
            model_rows: Vec::new(),
            model_list_state: ListState::default(),
            selected_model: None,
            trend_points: Vec::new(),
            model_sort: ModelSort::Tokens,
            detail_scope: DetailScope::All,
            detail_scroll: 0,
            detail_max_scroll: 0,
            detail_total_lines: 0,
            detail_viewport_height: 0,
            breakdown: false,
            search: SearchState::default(),
            period_input: PeriodInputState::default(),
            help_visible: false,
            footer_error: None,
            footer_notice: None,
            sessions_state: TabState::new(PaneFocus::Sessions),
            dates_state: TabState::new(PaneFocus::Dates),
            models_state: TabState::new(PaneFocus::Models),
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
    pub fn selected_model_position(&self) -> Option<usize> {
        let model = self.selected_model.as_ref()?;
        self.model_rows.iter().position(|r| r.model == *model)
    }
    pub fn selected_model_row(&self) -> Option<&ModelAggregateRow> {
        self.selected_model_position().map(|p| &self.model_rows[p])
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
            ViewTab::Sessions | ViewTab::Models => {
                self.snapshot.sessions.get(session_index).map(|s| &s.tokens)
            }
            ViewTab::Dates => self.selected_day_stats(session_index).map(|d| &d.tokens),
        }
    }
    pub fn display_session_tokens(&self, session_index: usize) -> Option<TokenStats> {
        match self.tab {
            ViewTab::Sessions | ViewTab::Models => self.filtered_session_tokens(session_index),
            ViewTab::Dates => self
                .selected_day_stats(session_index)
                .map(|d| d.tokens.clone()),
        }
    }
    pub fn filtered_session_detail(
        &self,
        session_index: usize,
    ) -> Option<(TokenStats, Vec<ModelStats>, u64)> {
        let today = Local::now().date_naive();
        self.filtered_session_detail_on(session_index, today)
    }
    fn filtered_session_detail_on(
        &self,
        session_index: usize,
        today: NaiveDate,
    ) -> Option<(TokenStats, Vec<ModelStats>, u64)> {
        let session = self.snapshot.sessions.get(session_index)?;
        if self.period_filter == PeriodFilter::All {
            return Some((
                session.tokens.clone(),
                session.models.clone(),
                session.record_count,
            ));
        }
        let mut total_tokens = TokenStats::default();
        let mut total_records = 0u64;
        type EffortMap = BTreeMap<Option<ReasoningEffort>, (TokenStats, u64)>;
        let mut by_model: BTreeMap<String, (TokenStats, u64, EffortMap)> = BTreeMap::new();
        for bucket in &session.daily {
            if bucket.record_count == 0 || !self.period_filter.matches_day(bucket.day, today) {
                continue;
            }
            add_tokens(&mut total_tokens, &bucket.tokens);
            total_records = total_records.saturating_add(bucket.record_count);
            for model in &bucket.models {
                let entry = by_model.entry(model.model.clone()).or_default();
                add_tokens(&mut entry.0, &model.tokens);
                entry.1 = entry.1.saturating_add(model.record_count);
                for effort in &model.efforts {
                    let eff = entry.2.entry(effort.effort.clone()).or_default();
                    add_tokens(&mut eff.0, &effort.tokens);
                    eff.1 = eff.1.saturating_add(effort.record_count);
                }
            }
        }
        if total_records == 0 {
            return None;
        }
        let mut models: Vec<ModelStats> = by_model
            .into_iter()
            .map(|(model, (tokens, record_count, efforts_map))| {
                let mut efforts: Vec<EffortStats> = efforts_map
                    .into_iter()
                    .map(|(effort, (et, ec))| EffortStats {
                        effort,
                        tokens: et,
                        record_count: ec,
                    })
                    .collect();
                efforts.sort_by(|a, b| {
                    b.tokens
                        .total_tokens
                        .cmp(&a.tokens.total_tokens)
                        .then_with(|| a.effort.cmp(&b.effort))
                });
                ModelStats {
                    model,
                    tokens,
                    efforts,
                    record_count,
                }
            })
            .collect();
        models.sort_by(|a, b| {
            b.tokens
                .total_tokens
                .cmp(&a.tokens.total_tokens)
                .then_with(|| a.model.cmp(&b.model))
        });
        Some((total_tokens, models, total_records))
    }
    pub fn client_date_tokens(&self, date_index: usize) -> Option<TokenStats> {
        let date = self.snapshot.dates.get(date_index)?;
        if self.client_filter == ClientScope::All {
            return Some(date.tokens.clone());
        }
        let mut sum = TokenStats::default();
        let mut records = 0u64;
        for item in &date.sessions {
            let session = self.snapshot.sessions.get(item.session_index)?;
            if !self.client_filter.matches(session.key.client) {
                continue;
            }
            if let Some(bucket) = session.daily.get(item.day_index)
                && bucket.record_count > 0
            {
                add_tokens(&mut sum, &bucket.tokens);
                records = records.saturating_add(bucket.record_count);
            }
        }
        (records > 0).then_some(sum)
    }
    pub fn filtered_session_tokens(&self, session_index: usize) -> Option<TokenStats> {
        let today = Local::now().date_naive();
        self.filtered_session_tokens_on(session_index, today)
    }
    fn filtered_session_tokens_on(
        &self,
        session_index: usize,
        today: NaiveDate,
    ) -> Option<TokenStats> {
        let session = self.snapshot.sessions.get(session_index)?;
        if self.period_filter == PeriodFilter::All {
            return Some(session.tokens.clone());
        }
        let mut sum = TokenStats::default();
        let mut records = 0u64;
        for bucket in &session.daily {
            if bucket.record_count > 0 && self.period_filter.matches_day(bucket.day, today) {
                add_tokens(&mut sum, &bucket.tokens);
                records = records.saturating_add(bucket.record_count);
            }
        }
        (records > 0).then_some(sum)
    }
    pub fn filtered_total_tokens(&self) -> u64 {
        let today = Local::now().date_naive();
        self.snapshot
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| self.client_filter.matches(s.key.client))
            .filter_map(|(i, _)| self.filtered_session_tokens_on(i, today))
            .fold(0u64, |acc, t| acc.saturating_add(t.total_tokens))
    }
    pub fn filtered_session_count(&self) -> usize {
        let today = Local::now().date_naive();
        self.snapshot
            .sessions
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                self.client_filter.matches(s.key.client)
                    && self.filtered_session_tokens_on(*i, today).is_some()
            })
            .count()
    }
    pub fn date_tokens(&self, date_index: usize) -> Option<TokenStats> {
        let date = self.snapshot.dates.get(date_index)?;
        match &self.pinned_session {
            Some(key) => {
                if !self.client_filter.matches(key.client) {
                    return None;
                }
                self.snapshot
                    .sessions
                    .iter()
                    .find(|s| s.key == *key)?
                    .daily
                    .iter()
                    .find(|d| d.day == date.day && d.record_count > 0)
                    .map(|d| d.tokens.clone())
            }
            None if self.client_filter == ClientScope::All => Some(date.tokens.clone()),
            None => {
                let mut sum = TokenStats::default();
                let mut records = 0u64;
                for item in &date.sessions {
                    let session = self.snapshot.sessions.get(item.session_index)?;
                    if !self.client_filter.matches(session.key.client) {
                        continue;
                    }
                    if let Some(bucket) = session.daily.get(item.day_index)
                        && bucket.record_count > 0
                    {
                        add_tokens(&mut sum, &bucket.tokens);
                        records = records.saturating_add(bucket.record_count);
                    }
                }
                (records > 0).then_some(sum)
            }
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
    pub fn set_detail_metrics(&mut self, total_lines: usize, viewport_height: usize) {
        self.detail_total_lines = total_lines;
        self.detail_viewport_height = viewport_height;
        self.detail_max_scroll = total_lines.saturating_sub(viewport_height);
        self.detail_scroll = self.detail_scroll.min(self.detail_max_scroll);
    }
    fn clamp_detail_scroll(&mut self) {
        if self.detail_total_lines > 0 {
            self.detail_scroll = self.detail_scroll.min(self.detail_max_scroll);
        }
    }
    pub fn cycle_client_filter(&mut self) {
        self.client_filter = self.client_filter.next();
        self.detail_scroll = 0;
        self.rebuild_filter();
    }
    pub fn cycle_period_filter(&mut self) {
        self.period_filter = self.period_filter.next();
        self.detail_scroll = 0;
        self.rebuild_filter();
    }
    pub fn start_period_input(&mut self) {
        self.period_input.editing = true;
        self.period_input.query = match self.period_filter {
            PeriodFilter::CustomDays(n) => n.to_string(),
            _ => String::new(),
        };
    }
    pub fn push_period_digit(&mut self, ch: char) {
        if ch.is_ascii_digit() && self.period_input.query.len() < 4 {
            self.period_input.query.push(ch);
        }
    }
    pub fn pop_period_digit(&mut self) {
        self.period_input.query.pop();
    }
    pub fn cancel_period_input(&mut self) {
        self.period_input = PeriodInputState::default();
    }
    pub fn confirm_period_input(&mut self) {
        let trimmed = self.period_input.query.trim();
        if trimmed.is_empty() {
            self.period_filter = PeriodFilter::All;
            self.detail_scroll = 0;
            self.rebuild_filter();
        } else if let Ok(n) = trimmed.parse::<u32>() {
            if n >= 1 {
                self.period_filter = PeriodFilter::CustomDays(n);
                self.detail_scroll = 0;
                self.rebuild_filter();
                self.footer_notice = Some(format!("Period filter: last {n}d"));
            } else {
                self.footer_notice = Some("Enter at least 1 day".into());
            }
        }
        self.period_input = PeriodInputState::default();
    }
    #[allow(dead_code)]
    pub fn session_period_totals(&self, session_index: usize) -> Option<SessionPeriodTotals> {
        self.session_period_totals_on(session_index, Local::now().date_naive())
    }
    pub fn session_period_totals_on(
        &self,
        session_index: usize,
        today: NaiveDate,
    ) -> Option<SessionPeriodTotals> {
        let session = self.snapshot.sessions.get(session_index)?;
        let custom_days = match self.period_filter {
            PeriodFilter::CustomDays(n) => Some(n),
            _ => None,
        };
        let mut today_sum = 0u64;
        let mut seven_sum = 0u64;
        let mut thirty_sum = 0u64;
        let mut custom_sum = 0u64;
        for bucket in &session.daily {
            if bucket.record_count == 0 {
                continue;
            }
            let tokens = bucket.tokens.total_tokens;
            if PeriodFilter::Today.matches_day(bucket.day, today) {
                today_sum = today_sum.saturating_add(tokens);
            }
            if PeriodFilter::SevenDays.matches_day(bucket.day, today) {
                seven_sum = seven_sum.saturating_add(tokens);
            }
            if PeriodFilter::ThirtyDays.matches_day(bucket.day, today) {
                thirty_sum = thirty_sum.saturating_add(tokens);
            }
            if let Some(n) = custom_days
                && PeriodFilter::CustomDays(n).matches_day(bucket.day, today)
            {
                custom_sum = custom_sum.saturating_add(tokens);
            }
        }
        Some(SessionPeriodTotals {
            today: today_sum,
            seven_days: seven_sum,
            thirty_days: thirty_sum,
            custom: custom_days.map(|n| (n, custom_sum)),
            all: session.tokens.total_tokens,
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
            PaneFocus::Models if self.tab == ViewTab::Models => self.move_model(delta),
            PaneFocus::Details => {
                self.detail_scroll = self.detail_scroll.saturating_add_signed(delta);
                self.clamp_detail_scroll();
            }
            _ => self.move_selection(delta),
        }
    }
    pub fn move_first(&mut self) {
        match self.focus {
            PaneFocus::Dates if self.tab == ViewTab::Dates => self.select_date_at(0),
            PaneFocus::Models if self.tab == ViewTab::Models => self.select_model_at(0),
            PaneFocus::Details => self.detail_scroll = 0,
            _ => self.select_session_at(0),
        }
    }
    pub fn move_last(&mut self) {
        match self.focus {
            PaneFocus::Dates if self.tab == ViewTab::Dates => {
                if !self.visible_dates.is_empty() {
                    self.select_date_at(self.visible_dates.len() - 1);
                }
            }
            PaneFocus::Models if self.tab == ViewTab::Models => {
                if !self.model_rows.is_empty() {
                    self.select_model_at(self.model_rows.len() - 1);
                }
            }
            PaneFocus::Details => {
                if self.detail_total_lines > 0 {
                    self.detail_scroll = self.detail_max_scroll;
                }
            }
            _ => {
                if !self.filtered_sessions.is_empty() {
                    self.select_session_at(self.filtered_sessions.len() - 1);
                }
            }
        }
    }
    pub fn scroll_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
        self.clamp_detail_scroll();
    }
    pub fn scroll_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }
    pub fn page_down(&mut self) {
        self.move_active(8);
    }
    pub fn page_up(&mut self) {
        self.move_active(-8);
    }
    pub fn toggle_breakdown(&mut self) {
        self.breakdown = !self.breakdown;
        self.detail_scroll = 0;
    }
    pub fn move_focus(&mut self, delta: isize) {
        let panes: &[PaneFocus] = match self.tab {
            ViewTab::Sessions => &[PaneFocus::Sessions, PaneFocus::Details],
            ViewTab::Dates => &[PaneFocus::Dates, PaneFocus::Sessions, PaneFocus::Details],
            ViewTab::Models => &[PaneFocus::Models, PaneFocus::Details],
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
            ViewTab::Models => &self.models_state,
        }
        .clone();
        self.tab = tab;
        self.selected_session = state.selected_session;
        self.sort = state.sort;
        self.focus = state.focus;
        self.session_list_state = state.list;
        self.detail_scroll = state.detail_scroll;
        self.detail_total_lines = 0;
        self.detail_viewport_height = 0;
        self.detail_max_scroll = 0;
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
            ViewTab::Models => self.models_state = state,
        }
    }
    pub fn toggle_sort(&mut self) {
        if self.tab == ViewTab::Models {
            self.model_sort = self.model_sort.next();
        } else {
            self.sort = match self.sort {
                SessionSort::LastUsed => SessionSort::Tokens,
                SessionSort::Tokens => SessionSort::LastUsed,
            };
        }
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
        self.select_date_at(p);
    }
    pub fn select_date_at(&mut self, position: usize) {
        let Some(&date_index) = self.visible_dates.get(position) else {
            return;
        };
        let scope = date_scope(self.snapshot.dates[date_index].day);
        if self.selected_date != Some(scope) {
            self.selected_date = Some(scope);
            self.detail_scroll = 0;
            if let Some(key) = &self.pinned_session {
                self.selected_session = Some(key.clone());
            }
            self.rebuild_filter();
        }
    }
    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.filtered_sessions.is_empty() {
            return;
        }
        let p = self
            .selected_filtered_position()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(self.filtered_sessions.len() - 1);
        self.select_session_at(p);
    }
    pub fn select_session_at(&mut self, position: usize) {
        let Some(&session_index) = self.filtered_sessions.get(position) else {
            return;
        };
        let key = self.snapshot.sessions[session_index].key.clone();
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
    pub(super) fn move_model(&mut self, delta: isize) {
        if self.model_rows.is_empty() {
            return;
        }
        let p = self
            .selected_model_position()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(self.model_rows.len() - 1);
        self.select_model_at(p);
    }
    pub fn select_model_at(&mut self, position: usize) {
        let Some(row) = self.model_rows.get(position) else {
            return;
        };
        if self.selected_model.as_deref() != Some(&row.model) {
            self.selected_model = Some(row.model.clone());
            self.detail_scroll = 0;
        }
        self.model_list_state.select(self.selected_model_position());
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
        let today = Local::now().date_naive();
        if self.pinned_session.as_ref().is_some_and(|key| {
            !self.snapshot.sessions.iter().enumerate().any(|(i, s)| {
                s.key == *key
                    && self.client_filter.matches(s.key.client)
                    && self.filtered_session_tokens_on(i, today).is_some()
                    && matches_session(s, &query)
            })
        }) {
            self.pinned_session = None;
            self.footer_notice = Some("Pinned session unavailable; unpinned".into());
        }
        self.visible_dates = self
            .snapshot
            .dates
            .iter()
            .enumerate()
            .filter(|(i, d)| {
                d.record_count > 0
                    && self.period_filter.matches_day(d.day, today)
                    && self.date_tokens(*i).is_some()
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
            .filter(|(i, s)| {
                self.client_filter.matches(s.key.client)
                    && self.filtered_session_tokens_on(*i, today).is_some()
                    && matches_session(s, &query)
            })
            .filter(|(i, _)| self.tab != ViewTab::Dates || self.selected_day_stats(*i).is_some())
            .map(|(i, _)| i)
            .collect();
        let mut indices = std::mem::take(&mut self.filtered_sessions);
        indices.sort_by(|&a, &b| {
            let sa = &self.snapshot.sessions[a];
            let sb = &self.snapshot.sessions[b];
            let primary = match self.sort {
                SessionSort::LastUsed => sb.last_used_at.cmp(&sa.last_used_at),
                SessionSort::Tokens => {
                    let tb = match self.tab {
                        ViewTab::Dates => self.selected_day_stats(b).map(|d| d.tokens.total_tokens),
                        _ => self
                            .filtered_session_tokens_on(b, today)
                            .map(|t| t.total_tokens),
                    };
                    let ta = match self.tab {
                        ViewTab::Dates => self.selected_day_stats(a).map(|d| d.tokens.total_tokens),
                        _ => self
                            .filtered_session_tokens_on(a, today)
                            .map(|t| t.total_tokens),
                    };
                    tb.cmp(&ta)
                }
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
            ViewTab::Sessions | ViewTab::Models => DetailScope::All,
            ViewTab::Dates => self.selected_date.unwrap_or(DetailScope::All),
        };
        self.rebuild_models_and_trends(&query, today);
    }
    fn rebuild_models_and_trends(&mut self, query: &str, today: NaiveDate) {
        struct ModelAccum {
            tokens: TokenStats,
            record_count: u64,
            clients: BTreeSet<Client>,
            efforts: BTreeMap<Option<ReasoningEffort>, (TokenStats, u64)>,
            top_sessions: Vec<(SessionKey, Option<String>, u64)>,
        }
        let mut model_map: BTreeMap<String, ModelAccum> = BTreeMap::new();
        let mut trend_map: BTreeMap<NaiveDate, (u64, u64)> = BTreeMap::new();

        for session in &self.snapshot.sessions {
            if !self.client_filter.matches(session.key.client) {
                continue;
            }
            let mut session_models: BTreeMap<String, (TokenStats, u64, Vec<&EffortStats>)> =
                BTreeMap::new();
            if self.period_filter == PeriodFilter::All {
                for model in &session.models {
                    if !query.is_empty() && !model.model.to_ascii_lowercase().contains(query) {
                        continue;
                    }
                    let entry = session_models.entry(model.model.clone()).or_default();
                    add_tokens(&mut entry.0, &model.tokens);
                    entry.1 = entry.1.saturating_add(model.record_count);
                    entry.2.extend(model.efforts.iter());
                }
                for bucket in &session.daily {
                    if let Some(day) = bucket.day
                        && bucket.record_count > 0
                    {
                        add_trend_bucket(&mut trend_map, day, bucket, query);
                    }
                }
            } else {
                for bucket in &session.daily {
                    if bucket.record_count == 0
                        || !self.period_filter.matches_day(bucket.day, today)
                    {
                        continue;
                    }
                    if let Some(day) = bucket.day {
                        add_trend_bucket(&mut trend_map, day, bucket, query);
                    }
                    for model in &bucket.models {
                        if !query.is_empty() && !model.model.to_ascii_lowercase().contains(query) {
                            continue;
                        }
                        let entry = session_models.entry(model.model.clone()).or_default();
                        add_tokens(&mut entry.0, &model.tokens);
                        entry.1 = entry.1.saturating_add(model.record_count);
                        entry.2.extend(model.efforts.iter());
                    }
                }
            }

            for (model_name, (tokens, record_count, efforts)) in session_models {
                if record_count == 0 {
                    continue;
                }
                let accum = model_map.entry(model_name).or_insert_with(|| ModelAccum {
                    tokens: TokenStats::default(),
                    record_count: 0,
                    clients: BTreeSet::new(),
                    efforts: BTreeMap::new(),
                    top_sessions: Vec::new(),
                });
                add_tokens(&mut accum.tokens, &tokens);
                accum.record_count = accum.record_count.saturating_add(record_count);
                accum.clients.insert(session.key.client);
                for eff in efforts {
                    let eff_entry = accum.efforts.entry(eff.effort.clone()).or_default();
                    add_tokens(&mut eff_entry.0, &eff.tokens);
                    eff_entry.1 = eff_entry.1.saturating_add(eff.record_count);
                }
                accum.top_sessions.push((
                    session.key.clone(),
                    session.name.clone(),
                    tokens.total_tokens,
                ));
            }
        }

        let mut rows: Vec<ModelAggregateRow> = model_map
            .into_iter()
            .map(|(model, mut accum)| {
                let mut efforts: Vec<EffortStats> = accum
                    .efforts
                    .into_iter()
                    .map(|(effort, (tokens, record_count))| EffortStats {
                        effort,
                        tokens,
                        record_count,
                    })
                    .collect();
                efforts.sort_by(|a, b| {
                    b.tokens
                        .total_tokens
                        .cmp(&a.tokens.total_tokens)
                        .then_with(|| effort_name(&a.effort).cmp(&effort_name(&b.effort)))
                });
                accum.top_sessions.sort_by(|a, b| {
                    b.2.cmp(&a.2)
                        .then_with(|| a.0.qualified().cmp(&b.0.qualified()))
                });
                let session_count = accum.top_sessions.len();
                accum.top_sessions.truncate(10);
                ModelAggregateRow {
                    model,
                    tokens: accum.tokens,
                    record_count: accum.record_count,
                    session_count,
                    clients: accum.clients.into_iter().collect(),
                    efforts,
                    top_sessions: accum.top_sessions,
                }
            })
            .collect();

        rows.sort_by(|a, b| match self.model_sort {
            ModelSort::Tokens => b
                .tokens
                .total_tokens
                .cmp(&a.tokens.total_tokens)
                .then_with(|| a.model.cmp(&b.model)),
            ModelSort::Sessions => b
                .session_count
                .cmp(&a.session_count)
                .then_with(|| b.tokens.total_tokens.cmp(&a.tokens.total_tokens))
                .then_with(|| a.model.cmp(&b.model)),
            ModelSort::Name => a.model.cmp(&b.model),
        });

        self.model_rows = rows;
        if self.selected_model_position().is_none() {
            let next_model = self.model_rows.first().map(|r| r.model.clone());
            if self.tab == ViewTab::Models && self.selected_model != next_model {
                self.detail_scroll = 0;
            }
            self.selected_model = next_model;
        }
        self.model_list_state.select(self.selected_model_position());

        self.trend_points = trend_map
            .into_iter()
            .map(|(day, (tokens, record_count))| DailyTrendPoint {
                day,
                tokens,
                record_count,
            })
            .collect();
    }
    pub fn replace_snapshot(&mut self, snapshot: AggregateSnapshot) {
        self.save_tab();
        let old_date = self.selected_date;
        let old_date_key = self.dates_state.selected_session.clone();
        let old_pin = self.pinned_session.clone();
        let old_key = self.selected_session.clone();
        let old_model = self.selected_model.clone();
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
            || (self.tab == ViewTab::Models && old_model != self.selected_model)
        {
            self.detail_scroll = 0;
        }
        // Reconcile the inactive views as well, so stale selections and scroll never reappear.
        let active = self.tab;
        for other in [ViewTab::Sessions, ViewTab::Dates, ViewTab::Models] {
            if other != active {
                self.switch_tab(other);
            }
        }
        self.switch_tab(active);
        if old_date_key.is_some()
            && old_date_key != self.dates_state.selected_session
            && old_date == self.selected_date
            && !notices.contains(&"Session gone; first available session")
        {
            notices.push("Session date gone; first available session");
        }
        if notices.is_empty() {
            self.footer_notice = Some(format!("Reloaded at {}", Local::now().format("%H:%M:%S")));
        } else {
            self.footer_notice = Some(notices.join("; "));
        }
    }
}
fn add_tokens(dst: &mut TokenStats, src: &TokenStats) {
    dst.input_uncached = dst.input_uncached.saturating_add(src.input_uncached);
    dst.input_total = dst.input_total.saturating_add(src.input_total);
    dst.cache_read = dst.cache_read.saturating_add(src.cache_read);
    dst.cache_write = dst.cache_write.saturating_add(src.cache_write);
    dst.output_total = dst.output_total.saturating_add(src.output_total);
    dst.reasoning_known = dst.reasoning_known.saturating_add(src.reasoning_known);
    dst.reasoning_has_unknown |= src.reasoning_has_unknown;
    dst.total_tokens = dst.total_tokens.saturating_add(src.total_tokens);
}
fn add_trend_bucket(
    trend_map: &mut BTreeMap<NaiveDate, (u64, u64)>,
    day: NaiveDate,
    bucket: &SessionDayStats,
    query: &str,
) {
    if query.is_empty() {
        let entry = trend_map.entry(day).or_default();
        entry.0 = entry.0.saturating_add(bucket.tokens.total_tokens);
        entry.1 = entry.1.saturating_add(bucket.record_count);
    } else {
        for model in &bucket.models {
            if model.model.to_ascii_lowercase().contains(query) && model.record_count > 0 {
                let entry = trend_map.entry(day).or_default();
                entry.0 = entry.0.saturating_add(model.tokens.total_tokens);
                entry.1 = entry.1.saturating_add(model.record_count);
            }
        }
    }
}
fn effort_name(effort: &Option<ReasoningEffort>) -> String {
    effort
        .as_ref()
        .map(|e| e.label())
        .unwrap_or_else(|| "none".into())
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
            workspace: None,
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
            workspace: None,
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
    fn parsed_subagents_show_only_roots_in_both_tabs_search_and_refresh() {
        use crate::{aggregate::aggregate, sources};
        use serde_json::json;
        for client in [Gjc, Codex] {
            let dir = tempfile::tempdir().unwrap();
            let logs = if client == Codex {
                dir.path().join("sessions")
            } else {
                dir.path().to_path_buf()
            };
            std::fs::create_dir_all(&logs).unwrap();
            let root = if client == Codex {
                std::fs::write(
                    dir.path().join("session_index.jsonl"),
                    "{\"id\":\"root\",\"name\":\"Root title\"}\n",
                )
                .unwrap();
                json!({"type":"session_meta","payload":{"id":"root"}})
            } else {
                json!({"type":"header","id":"root","title":"Root title"})
            };
            std::fs::write(logs.join("root.jsonl"), format!("{root}\n")).unwrap();
            let snapshot = |extra: bool| {
                let mut lines = if client == Codex {
                    vec![
                        json!({"type":"session_meta","payload":{"id":"child","parent_thread_id":"root"}}),
                        json!({"type":"turn_context","payload":{"model":"child-model"}}),
                    ]
                } else {
                    vec![
                        json!({"type":"header","id":"child","parent_session_id":"root","title":"Child title"}),
                    ]
                };
                for (index, (timestamp, total)) in
                    [("2026-03-10T12:00:00Z", 11), ("2026-03-11T12:00:00Z", 17)]
                        .into_iter()
                        .enumerate()
                {
                    if index == 1 && !extra {
                        continue;
                    }
                    lines.push(if client == Codex {
                        json!({"type":"event_msg","timestamp":timestamp,"payload":{"type":"token_count","info":{
                            "total_token_usage":{"total_tokens":if index == 0 { 11 } else { 28 }},
                            "last_token_usage":{"input_tokens":total-1,"output_tokens":1,"total_tokens":total}}}})
                    } else {
                        json!({"type":"message","id":format!("m{index}"),"timestamp":timestamp,
                            "message":{"role":"assistant","model":"child-model","usage":{"input":total-1,"output":1,"totalTokens":total}}})
                    });
                }
                std::fs::write(
                    logs.join("child.jsonl"),
                    lines.iter().map(|v| format!("{v}\n")).collect::<String>(),
                )
                .unwrap();
                let parsed = if client == Codex {
                    sources::codex::parse(dir.path())
                } else {
                    sources::gjc::parse(dir.path())
                }
                .unwrap();
                aggregate(vec![parsed]).unwrap()
            };
            let mut app = App::new(snapshot(false));
            assert_eq!(ids(&app), ["root"]);
            assert_eq!(
                app.selected_session().unwrap().name.as_deref(),
                Some("Root title")
            );
            assert_eq!(app.selected_session().unwrap().tokens.total_tokens, 11);
            app.switch_tab(ViewTab::Dates);
            assert_eq!(ids(&app), ["root"]);
            let selected_date = app.selected_date;
            let selected_key = app.selected_session.clone();
            app.replace_snapshot(snapshot(true));
            assert_eq!(app.selected_date, selected_date);
            assert_eq!(app.selected_session, selected_key);
            assert_eq!(
                app.selected_tokens(app.selected_index().unwrap())
                    .unwrap()
                    .total_tokens,
                11
            );
            app.switch_tab(ViewTab::Sessions);
            assert_eq!(ids(&app), ["root"]);
            assert_eq!(app.selected_session().unwrap().tokens.total_tokens, 28);
            assert_eq!(
                app.selected_session().unwrap().last_used_at,
                Some("2026-03-11T12:00:00Z".parse().unwrap())
            );
            app.start_search();
            for c in "child".chars() {
                app.push_search_char(c);
            }
            app.confirm_search();
            assert!(app.filtered_sessions.is_empty());
            app.cancel_search();
            app.start_search();
            for c in "root title".chars() {
                app.push_search_char(c);
            }
            app.confirm_search();
            assert_eq!(ids(&app), ["root"]);
        }
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
        assert!(
            app.footer_notice
                .as_deref()
                .is_some_and(|n| n.starts_with("Reloaded at "))
        );
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

    #[test]
    fn client_and_period_filters_update_sessions_dates_models_and_trends() {
        use crate::{
            aggregate::aggregate,
            domain::{Client, ReasoningEffort, SessionId, UsageRecord},
            scan::ParseResult,
        };
        let today = Local::now().date_naive();
        let today_iso = format!("{today}T10:00:00Z");
        let five_days_ago = (today - Duration::days(5))
            .format("%Y-%m-%dT10:00:00Z")
            .to_string();
        let forty_days_ago = (today - Duration::days(40))
            .format("%Y-%m-%dT10:00:00Z")
            .to_string();

        let mut result = ParseResult::empty(Client::Gjc, "filters".into());
        for (client, id, ts, model, tokens) in [
            (
                Client::Codex,
                "c-today",
                Some(today_iso.as_str()),
                "gpt-5",
                100,
            ),
            (
                Client::Gjc,
                "g-5d",
                Some(five_days_ago.as_str()),
                "gemini-2.5",
                200,
            ),
            (
                Client::OpenCode,
                "o-40d",
                Some(forty_days_ago.as_str()),
                "claude-4",
                400,
            ),
            (Client::OpenCode, "o-undated", None, "claude-4", 800),
        ] {
            result.records.push(UsageRecord {
                session_key: SessionKey {
                    client,
                    id: SessionId(id.into()),
                },
                parent_session_key: None,
                message_id: None,
                source_path: "filters".into(),
                source_line: None,
                started_at: ts.map(|t| t.parse().unwrap()),
                session_name: Some(id.into()),
                workspace: None,
                model: model.into(),
                reasoning_effort: Some(ReasoningEffort::High),
                tokens: TokenStats {
                    total_tokens: tokens,
                    input_total: tokens,
                    ..Default::default()
                },
            });
        }
        let mut app = App::new(aggregate(vec![result]).unwrap());
        assert_eq!(app.filtered_session_count(), 4);
        assert_eq!(app.filtered_total_tokens(), 1500);
        assert_eq!(app.model_rows.len(), 3);
        assert_eq!(app.trend_points.len(), 3);

        // Cycle client filter: All -> Codex -> Gjc -> OpenCode -> All
        app.cycle_client_filter();
        assert_eq!(app.client_filter, ClientScope::Codex);
        assert_eq!(app.filtered_session_count(), 1);
        assert_eq!(app.filtered_total_tokens(), 100);
        assert_eq!(app.model_rows.len(), 1);
        assert_eq!(app.model_rows[0].model, "gpt-5");
        assert_eq!(app.visible_dates.len(), 1);
        assert_eq!(
            app.date_tokens(app.visible_dates[0]).unwrap().total_tokens,
            100
        );

        app.cycle_client_filter();
        assert_eq!(app.client_filter, ClientScope::Gjc);
        assert_eq!(app.filtered_total_tokens(), 200);

        app.cycle_client_filter();
        assert_eq!(app.client_filter, ClientScope::OpenCode);
        assert_eq!(app.filtered_session_count(), 2);
        assert_eq!(app.filtered_total_tokens(), 1200);
        assert_eq!(app.model_rows[0].session_count, 2);

        app.cycle_client_filter();
        assert_eq!(app.client_filter, ClientScope::All);

        // Cycle period filter: All -> Today -> 7d -> 30d -> All
        app.cycle_period_filter();
        assert_eq!(app.period_filter, PeriodFilter::Today);
        assert_eq!(app.filtered_session_count(), 1);
        assert_eq!(app.filtered_total_tokens(), 100);
        assert_eq!(app.visible_dates.len(), 1);
        assert_eq!(app.trend_points.len(), 1);

        app.cycle_period_filter();
        assert_eq!(app.period_filter, PeriodFilter::SevenDays);
        assert_eq!(app.filtered_session_count(), 2);
        assert_eq!(app.filtered_total_tokens(), 300);
        assert_eq!(app.visible_dates.len(), 2);

        app.cycle_period_filter();
        assert_eq!(app.period_filter, PeriodFilter::ThirtyDays);
        assert_eq!(app.filtered_total_tokens(), 300);

        app.cycle_period_filter();
        assert_eq!(app.period_filter, PeriodFilter::All);
        assert_eq!(app.filtered_total_tokens(), 1500);

        // Preserves selected_model across filter changes
        app.switch_tab(ViewTab::Models);
        app.select_model_at(1);
        let chosen = app.selected_model.clone();
        assert!(chosen.is_some());
        app.cycle_period_filter(); // Today (only gpt-5)
        app.cycle_period_filter(); // 7d (gpt-5 + gemini-2.5)
        app.cycle_period_filter(); // 30d
        app.cycle_period_filter(); // All
        assert_eq!(app.model_rows.len(), 3);
    }

    #[test]
    fn quick_nav_paging_detail_clamping_and_initial_session() {
        let rows = (0..12)
            .map(|i| {
                let id = format!("s{i:02}");
                // Leak or convert? Use static? No, test_snapshot accepts &str, build Vec<String> first
                id
            })
            .collect::<Vec<_>>();
        let refs = rows
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), Gjc, None, (i as u64 + 1) * 10, true))
            .collect::<Vec<_>>();
        let snapshot = test_snapshot(&refs);
        let target_key = SessionKey {
            client: Gjc,
            id: crate::domain::SessionId("s05".into()),
        };
        let mut app = App::with_initial_session(snapshot, Some(target_key.clone()));
        assert_eq!(app.selected_session, Some(target_key));

        app.move_first();
        assert_eq!(app.selected_filtered_position(), Some(0));
        app.page_down();
        assert_eq!(app.selected_filtered_position(), Some(8));
        app.move_last();
        assert_eq!(app.selected_filtered_position(), Some(11));
        app.page_up();
        assert_eq!(app.selected_filtered_position(), Some(3));

        app.focus = PaneFocus::Details;
        app.set_detail_metrics(20, 10);
        assert_eq!(app.detail_max_scroll, 10);
        app.page_down();
        assert_eq!(app.detail_scroll, 8);
        app.page_down();
        assert_eq!(app.detail_scroll, 10);
        app.scroll_down();
        assert_eq!(app.detail_scroll, 10);
        app.move_first();
        assert_eq!(app.detail_scroll, 0);
        app.move_last();
        assert_eq!(app.detail_scroll, 10);
    }

    #[test]
    fn custom_days_period_filter_matches_boundaries_and_cycles_to_all() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let d3 = PeriodFilter::CustomDays(3);
        assert_eq!(d3.label(), "3d");
        assert_eq!(d3.next(), PeriodFilter::All);
        assert!(d3.matches_day(Some(today), today));
        assert!(d3.matches_day(Some(today - Duration::days(2)), today));
        assert!(!d3.matches_day(Some(today - Duration::days(3)), today));
        assert!(!d3.matches_day(Some(today + Duration::days(1)), today));
        assert!(!d3.matches_day(None, today));

        let d1 = PeriodFilter::CustomDays(1);
        assert!(d1.matches_day(Some(today), today));
        assert!(!d1.matches_day(Some(today - Duration::days(1)), today));
        assert!(!PeriodFilter::CustomDays(0).matches_day(Some(today), today));
    }

    #[test]
    fn period_input_confirm_cancel_zero_and_empty() {
        let today = Local::now().date_naive();
        let two_days_ago = (today - Duration::days(2))
            .format("%Y-%m-%dT10:00:00Z")
            .to_string();
        let ten_days_ago = (today - Duration::days(10))
            .format("%Y-%m-%dT10:00:00Z")
            .to_string();
        let mut app = App::new(test_snapshot(&[
            ("s-2d", Gjc, Some(two_days_ago.as_str()), 200, true),
            ("s-10d", Gjc, Some(ten_days_ago.as_str()), 500, true),
        ]));

        // Confirm valid custom days
        app.detail_scroll = 5;
        app.start_period_input();
        assert!(app.period_input.editing);
        assert!(app.period_input.query.is_empty());
        app.push_period_digit('a'); // ignored
        for ch in "12345".chars() {
            app.push_period_digit(ch);
        }
        assert_eq!(app.period_input.query, "1234");
        app.pop_period_digit();
        app.pop_period_digit();
        app.pop_period_digit();
        app.pop_period_digit();
        app.push_period_digit('5');
        app.confirm_period_input();
        assert!(!app.period_input.editing);
        assert!(app.period_input.query.is_empty());
        assert_eq!(app.period_filter, PeriodFilter::CustomDays(5));
        assert_eq!(app.detail_scroll, 0);
        assert_eq!(app.filtered_session_count(), 1);
        assert_eq!(app.filtered_total_tokens(), 200);
        assert_eq!(app.footer_notice.as_deref(), Some("Period filter: last 5d"));

        // Start while CustomDays(5) pre-fills "5", cancel preserves filter
        app.start_period_input();
        assert_eq!(app.period_input.query, "5");
        app.push_period_digit('0');
        app.cancel_period_input();
        assert!(!app.period_input.editing);
        assert!(app.period_input.query.is_empty());
        assert_eq!(app.period_filter, PeriodFilter::CustomDays(5));

        // Zero days shows notice and preserves existing filter
        app.start_period_input();
        app.pop_period_digit();
        app.push_period_digit('0');
        app.confirm_period_input();
        assert!(!app.period_input.editing);
        assert!(app.period_input.query.is_empty());
        assert_eq!(app.period_filter, PeriodFilter::CustomDays(5));
        assert_eq!(app.footer_notice.as_deref(), Some("Enter at least 1 day"));

        // Empty input resets to All
        app.start_period_input();
        app.pop_period_digit();
        app.confirm_period_input();
        assert!(!app.period_input.editing);
        assert_eq!(app.period_filter, PeriodFilter::All);
        assert_eq!(app.filtered_session_count(), 2);
        assert_eq!(app.filtered_total_tokens(), 700);
    }

    #[test]
    fn session_period_totals_on_computes_buckets_and_optional_custom() {
        let mut app = App::new(test_snapshot(&[
            ("multi", Gjc, Some("2026-03-15T12:00:00Z"), 10, true),
            ("multi", Gjc, Some("2026-03-09T12:00:00Z"), 20, true),
            ("multi", Gjc, Some("2026-03-02T12:00:00Z"), 40, true),
            ("multi", Gjc, Some("2026-02-14T12:00:00Z"), 80, true),
            ("multi", Gjc, Some("2026-02-01T12:00:00Z"), 160, true),
            ("multi", Gjc, None, 320, true),
        ]));
        let today = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let totals = app.session_period_totals_on(0, today).unwrap();
        assert_eq!(
            totals,
            SessionPeriodTotals {
                today: 10,
                seven_days: 30,
                thirty_days: 150,
                custom: None,
                all: 630,
            }
        );

        app.period_filter = PeriodFilter::CustomDays(14);
        let totals_14d = app.session_period_totals_on(0, today).unwrap();
        assert_eq!(
            totals_14d,
            SessionPeriodTotals {
                today: 10,
                seven_days: 30,
                thirty_days: 150,
                custom: Some((14, 70)),
                all: 630,
            }
        );
        assert!(app.session_period_totals_on(usize::MAX, today).is_none());
    }
}
