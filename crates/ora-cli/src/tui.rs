use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ora::{
    AdminClient, JobOrderBy, ScheduleOrderBy,
    execution::ExecutionStatus as FilterExecutionStatus,
    proto::admin::v1::{ExecutionStatus, ScheduleStatus as ProtoScheduleStatus},
};
use ratatui::DefaultTerminal;
use tokio::{spawn, task::JoinHandle};

use crate::tui::{
    events::{AppEvent, Events, Request},
    ui::JobTypeList,
};

/// How often the loading indicator advances.
const TICK_INTERVAL: Duration = Duration::from_millis(120);

/// How often data is refreshed in the background.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

mod data;
mod events;
mod ui;

pub(crate) async fn run(client: AdminClient) -> eyre::Result<()> {
    let terminal = ratatui::init();

    // Without this a paste arrives as the keys it is made of, and the
    // newline at the end of one submits the form it was pasted into.
    let bracketed = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::EnableBracketedPaste
    )
    .is_ok();

    let result = App::new(client).run(terminal).await;

    if bracketed {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableBracketedPaste
        );
    }

    ratatui::restore();
    result
}

/// The top-level views of the application.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Jobs,
    Schedules,
    Executors,
}

impl Tab {
    fn next(self) -> Self {
        match self {
            Tab::Jobs => Tab::Schedules,
            Tab::Schedules => Tab::Executors,
            Tab::Executors => Tab::Jobs,
        }
    }

    fn index(self) -> usize {
        match self {
            Tab::Jobs => 0,
            Tab::Schedules => 1,
            Tab::Executors => 2,
        }
    }

    fn previous(self) -> Self {
        match self {
            Tab::Jobs => Tab::Executors,
            Tab::Schedules => Tab::Jobs,
            Tab::Executors => Tab::Schedules,
        }
    }
}

/// The job status rows are filtered by, mirroring `--status`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum JobStatus {
    #[default]
    All,
    Active,
    Pending,
    InProgress,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    fn next(self) -> Self {
        match self {
            JobStatus::All => JobStatus::Active,
            JobStatus::Active => JobStatus::Pending,
            JobStatus::Pending => JobStatus::InProgress,
            JobStatus::InProgress => JobStatus::Succeeded,
            JobStatus::Succeeded => JobStatus::Failed,
            JobStatus::Failed => JobStatus::Cancelled,
            JobStatus::Cancelled => JobStatus::All,
        }
    }

    /// The statuses sent to the server, `None` meaning no filter.
    pub(crate) fn statuses(self) -> Option<Vec<FilterExecutionStatus>> {
        match self {
            JobStatus::All => None,
            JobStatus::Active => Some(vec![
                FilterExecutionStatus::Pending,
                FilterExecutionStatus::InProgress,
            ]),
            JobStatus::Pending => Some(vec![FilterExecutionStatus::Pending]),
            JobStatus::InProgress => Some(vec![FilterExecutionStatus::InProgress]),
            JobStatus::Succeeded => Some(vec![FilterExecutionStatus::Succeeded]),
            JobStatus::Failed => Some(vec![FilterExecutionStatus::Failed]),
            JobStatus::Cancelled => Some(vec![FilterExecutionStatus::Cancelled]),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            JobStatus::All => "all",
            JobStatus::Active => "active",
            JobStatus::Pending => "pending",
            JobStatus::InProgress => "in-progress",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }
}

/// The schedule status rows are filtered by, mirroring `--status`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ScheduleStatus {
    #[default]
    All,
    Active,
    Stopped,
}

impl ScheduleStatus {
    fn next(self) -> Self {
        match self {
            ScheduleStatus::All => ScheduleStatus::Active,
            ScheduleStatus::Active => ScheduleStatus::Stopped,
            ScheduleStatus::Stopped => ScheduleStatus::All,
        }
    }

    pub(crate) fn statuses(self) -> Option<Vec<ora::ScheduleStatus>> {
        match self {
            ScheduleStatus::All => None,
            ScheduleStatus::Active => Some(vec![ora::ScheduleStatus::Active]),
            ScheduleStatus::Stopped => Some(vec![ora::ScheduleStatus::Stopped]),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            ScheduleStatus::All => "all",
            ScheduleStatus::Active => "active",
            ScheduleStatus::Stopped => "stopped",
        }
    }
}

/// How close to the last loaded row the selection has to come before
/// the next page is fetched.
const PAGE_AHEAD: usize = 5;

/// How many pages are loaded without being asked for.
///
/// The first fills the table early, the rest follow in the background.
/// Pages past these come from scrolling and stop the table refreshing
/// on its own, which would drop them.
const AUTO_PAGES: usize = 2;

/// An action that is only carried out
/// once the user confirms it.
#[derive(Debug)]
pub(crate) struct Confirm {
    pub(crate) prompt: String,
    action: ConfirmAction,
}

#[derive(Debug)]
enum ConfirmAction {
    CancelJob(String),
    StopSchedule(String),
    Create(Box<Created>),
}

/// The outcome of the most recent background request.
#[derive(Debug, Default)]
pub(crate) struct Status {
    pub(crate) error: Option<String>,
    pub(crate) updated_at: Option<Instant>,
}

/// The request of one kind in flight, if any, and since when.
///
/// Holds the task so that starting a request drops the one it
/// replaces, rather than leaving a query running for every job type
/// jumped over on the way here.
#[derive(Debug, Default)]
pub(crate) struct Counter {
    task: Option<JoinHandle<()>>,
    since: Option<Instant>,
    /// Bumped on every start/cancel, so an answer from a superseded
    /// request can be told apart from the current one even if its
    /// `JoinHandle::abort()` didn't land before it was sent.
    token: u64,
}

impl Counter {
    fn next_token(&mut self) -> u64 {
        self.token += 1;
        self.token
    }

    fn start(&mut self, task: JoinHandle<()>) {
        if let Some(previous) = self.task.replace(task) {
            previous.abort();
        }

        self.since = Some(Instant::now());
    }

    fn finish(&mut self) {
        self.task = None;
        self.since = None;
    }

    /// Drop the request in flight without waiting for its answer.
    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }

        self.since = None;
        self.token += 1;
    }

    fn idle(&self) -> bool {
        self.since.is_none()
    }

    /// How long the request of this kind has been waiting.
    pub(crate) fn waited(&self) -> Option<Duration> {
        self.since.map(|since| since.elapsed())
    }

    /// Whether an answer's token is the one currently awaited.
    fn accepts(&self, token: u64) -> bool {
        token == self.token
    }
}

/// The requests in flight, by kind.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    job_types: Counter,
    jobs: Counter,
    schedules: Counter,
    executors: Counter,
    executor_jobs: Counter,
}

impl Pending {
    fn counters(&self) -> [&Counter; 5] {
        [
            &self.job_types,
            &self.jobs,
            &self.schedules,
            &self.executors,
            &self.executor_jobs,
        ]
    }

    /// How long the longest outstanding request has been waiting.
    pub(crate) fn waited(&self) -> Option<Duration> {
        self.counters()
            .into_iter()
            .filter_map(Counter::waited)
            .max()
    }

    /// Whether the table backing the given tab is waiting for data.
    pub(crate) fn for_tab(&self, tab: Tab) -> bool {
        match tab {
            Tab::Jobs => !self.jobs.idle(),
            Tab::Schedules => !self.schedules.idle(),
            Tab::Executors => !self.executors.idle(),
        }
    }

    fn finish(&mut self, request: Request) {
        let counter = match request {
            Request::JobTypes => &mut self.job_types,
            Request::Jobs => &mut self.jobs,
            Request::Schedules => &mut self.schedules,
            Request::Executors => &mut self.executors,
            Request::ExecutorJobs => &mut self.executor_jobs,
            Request::Action => return,
        };

        counter.finish();
    }
}

#[derive(Debug)]
pub struct App {
    client: AdminClient,
    running: bool,
    events: Events,
    tab: Tab,
    job_type_list: ui::JobTypeList,
    job_table: ui::JobTable,
    schedule_table: ui::ScheduleTable,
    executor_table: ui::ExecutorTable,
    /// Jobs of the executor whose detail view is open.
    executor_jobs: Vec<ora::proto::admin::v1::Job>,
    /// The detail view of each tab, kept so that switching tabs
    /// and coming back returns to where you were.
    details: [Option<ui::Detail>; 3],
    confirm: Option<Confirm>,
    /// The creation form, when one is open.
    form: Option<ui::Form>,
    status: Status,
    pending: Pending,
    spinner: usize,
    job_order: JobOrderBy,
    schedule_order: ScheduleOrderBy,
    labels_focused: bool,
    /// The label filter of each tab, which they do not share.
    label_filters: [String; 3],
    job_status: JobStatus,
    schedule_status: ScheduleStatus,
}

impl App {
    /// Construct a new instance of [`App`].
    pub fn new(client: AdminClient) -> Self {
        Self {
            client,
            running: false,
            tab: Tab::default(),
            job_type_list: JobTypeList::default(),
            job_table: ui::JobTable::default(),
            schedule_table: ui::ScheduleTable::default(),
            executor_table: ui::ExecutorTable::default(),
            executor_jobs: Vec::new(),
            details: [const { None }; 3],
            confirm: None,
            form: None,
            status: Status::default(),
            pending: Pending::default(),
            spinner: 0,
            events: Events::default(),
            job_order: JobOrderBy::CreatedAtDesc,
            schedule_order: ScheduleOrderBy::CreatedAtDesc,
            labels_focused: false,
            label_filters: Default::default(),
            job_status: JobStatus::default(),
            schedule_status: ScheduleStatus::default(),
        }
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> eyre::Result<()> {
        spawn({
            let events = self.events.sender();

            async move {
                let mut interval = tokio::time::interval(REFRESH_INTERVAL);
                loop {
                    interval.tick().await;
                    let _ = events.send_async(AppEvent::Refresh).await;
                }
            }
        });

        spawn({
            let events = self.events.sender();

            async move {
                let mut interval = tokio::time::interval(TICK_INTERVAL);
                loop {
                    interval.tick().await;
                    let _ = events.send_async(AppEvent::Tick).await;
                }
            }
        });

        self.job_type_list.focused = true;
        self.fetch(true);
        self.running = true;
        terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
        while self.running {
            if self.handle_events().await? {
                terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            }
        }
        Ok(())
    }

    /// Handle the next event, returning whether it changed anything
    /// worth redrawing for.
    async fn handle_events(&mut self) -> eyre::Result<bool> {
        let Some(event) = self.events.next().await else {
            return Ok(false);
        };

        // Only the spinner animates on its own; a tick redraws
        // nothing while there is nothing pending to show one for.
        if let AppEvent::Tick = event {
            self.spinner = self.spinner.wrapping_add(1);
            return Ok(self.pending.waited().is_some());
        }

        match event {
            AppEvent::Term(event) => match event {
                Event::Key(key_event) => self.on_key(key_event),
                Event::Paste(text) => self.on_paste(&text),
                _ => {}
            },
            AppEvent::JobTypesUpdated(job_type_infos) => {
                self.pending.finish(Request::JobTypes);

                // The selection is an index, so it has to follow
                // the job type it was on. One appearing above it
                // would otherwise leave it on a neighbour, with
                // the rows of the type before it still listed.
                let selected = self.selected_job_type();
                self.job_type_list.job_types = job_type_infos;
                self.answered();

                if let Some(selected) = selected {
                    let moved = self
                        .job_type_list
                        .job_types
                        .iter()
                        .position(|job_type| job_type.id == selected);

                    self.job_type_list.state.select(moved);
                }

                let prev_selection = self.job_type_list.state.selected();

                if prev_selection.is_none() {
                    self.job_type_list.state.select_next();
                    self.job_type_selected(prev_selection, self.job_type_list.state.selected());
                }
            }
            AppEvent::JobsUpdated(token, jobs, next_page, append) => {
                if self.pending.jobs.accepts(token) {
                    self.pending.jobs.finish();

                    if append {
                        self.job_table.incoming.extend(jobs);
                        self.job_table.pages += 1;
                    } else {
                        self.job_table.incoming = jobs;
                        self.job_table.pages = 1;
                    }

                    self.job_table.next_page = next_page;

                    // A refresh starts over at the first page, so
                    // showing each as it lands blinks the later
                    // ones out. Wait, unless the table is empty
                    // and there is nothing to lose by not waiting.
                    let more =
                        self.job_table.pages < AUTO_PAGES && self.job_table.next_page.is_some();

                    if !more || self.job_table.jobs.is_empty() {
                        self.job_table.jobs.clone_from(&self.job_table.incoming);
                    }

                    self.data_updated();
                    self.refresh_detail();

                    if more {
                        self.load_more_jobs();
                    }
                }
            }
            AppEvent::SchedulesUpdated(token, schedules, next_page, append) => {
                if self.pending.schedules.accepts(token) {
                    self.pending.schedules.finish();

                    if append {
                        self.schedule_table.incoming.extend(schedules);
                        self.schedule_table.pages += 1;
                    } else {
                        self.schedule_table.incoming = schedules;
                        self.schedule_table.pages = 1;
                    }

                    self.schedule_table.next_page = next_page;

                    let more = self.schedule_table.pages < AUTO_PAGES
                        && self.schedule_table.next_page.is_some();

                    if !more || self.schedule_table.schedules.is_empty() {
                        self.schedule_table
                            .schedules
                            .clone_from(&self.schedule_table.incoming);
                    }

                    self.data_updated();
                    self.refresh_detail();

                    if more {
                        self.load_more_schedules();
                    }
                }
            }
            AppEvent::ExecutorsUpdated(executors) => {
                self.pending.finish(Request::Executors);
                self.executor_table.executors = executors;
                self.data_updated();
                self.refresh_detail();
            }
            AppEvent::ExecutorJobsUpdated(executor_id, jobs) => {
                if self.detail().is_some_and(|detail| detail.id == executor_id) {
                    self.pending.finish(Request::ExecutorJobs);
                    self.executor_jobs = jobs;
                    self.data_updated();
                    self.refresh_detail();
                }
            }
            AppEvent::Created => {
                self.form = None;
                self.reload();
            }
            AppEvent::CreateFailed(error) => {
                if let Some(form) = self.form.as_mut() {
                    form.submitting = false;
                    form.error = Some(error);
                }
            }
            // A request made for filters that have since been
            // replaced answers a question nobody is asking any
            // more, and it must not clear the wait for the one
            // that replaced it either.
            AppEvent::Failed(request, error, token) => {
                let accepted = match (request, token) {
                    (Request::Jobs, Some(token)) => self.pending.jobs.accepts(token),
                    (Request::Schedules, Some(token)) => self.pending.schedules.accepts(token),
                    _ => true,
                };

                if accepted {
                    self.pending.finish(request);
                    self.status.error = Some(error);
                }
            }
            AppEvent::Tick => unreachable!("handled above"),
            AppEvent::Refresh => self.fetch(false),
        }

        Ok(true)
    }

    /// Take pasted text wherever typing would go.
    ///
    /// Control characters are dropped, since every field here holds a
    /// single line and the bindings are not meant to see them.
    fn on_paste(&mut self, text: &str) {
        if self.confirm.is_some() {
            return;
        }

        for c in text.chars().filter(|c| !c.is_control()) {
            if let Some(form) = self.form.as_mut() {
                form.push_char(c);
            } else if self.labels_focused {
                self.label_filter_mut().push(c);
            } else {
                return;
            }
        }
    }

    fn on_key(&mut self, event: KeyEvent) {
        match (event.modifiers, event.code, event.kind) {
            // The confirmation sits on top of everything, including the
            // form, so it has to be matched before it.
            (KeyModifiers::NONE, key, KeyEventKind::Press) if self.confirm.is_some() => match key {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.run_confirmed_action(),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => self.confirm = None,
                _ => {}
            },
            (KeyModifiers::NONE | KeyModifiers::SHIFT, key, KeyEventKind::Press)
                if self.form.is_some() =>
            {
                if key == KeyCode::Enter {
                    self.submit_form();
                    return;
                }

                let Some(form) = self.form.as_mut() else {
                    return;
                };

                match key {
                    KeyCode::Esc => self.form = None,
                    KeyCode::Up => form.select_previous(),
                    KeyCode::Down => form.select_next(),
                    KeyCode::Tab => form.select_next_cell(),
                    KeyCode::BackTab => form.select_previous_cell(),
                    KeyCode::Left => form.cycle(false),
                    KeyCode::Right => form.cycle(true),
                    KeyCode::Backspace => form.pop_char(),
                    KeyCode::Delete => form.delete_char(),
                    KeyCode::Char(c) => form.push_char(c),
                    _ => {}
                }
            }
            // Only these keys are taken, so ctrl-c still quits from a form.
            (
                KeyModifiers::ALT | KeyModifiers::CONTROL,
                key @ (KeyCode::Left | KeyCode::Right | KeyCode::Backspace),
                KeyEventKind::Press,
            ) if self.form.is_some() => {
                let Some(form) = self.form.as_mut() else {
                    return;
                };

                match key {
                    KeyCode::Left => form.move_word(false),
                    KeyCode::Right => form.move_word(true),
                    _ => form.pop_word(),
                }
            }
            (KeyModifiers::NONE, KeyCode::Tab, KeyEventKind::Press) if !self.labels_focused => {
                self.switch_tab(true);
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab, KeyEventKind::Press)
                if !self.labels_focused =>
            {
                self.switch_tab(false);
            }
            (KeyModifiers::NONE, key, KeyEventKind::Press) if self.detail().is_some() => {
                let Some(detail) = self.detail_mut().as_mut() else {
                    return;
                };

                match key {
                    KeyCode::Esc | KeyCode::Enter => self.close_detail(),
                    KeyCode::Left => detail.select_previous_page(),
                    KeyCode::Right => detail.select_next_page(),
                    KeyCode::Up => detail.scroll_up(1),
                    KeyCode::Down => detail.scroll_down(1),
                    KeyCode::PageUp => detail.scroll_up(20),
                    KeyCode::PageDown => detail.scroll_down(20),
                    KeyCode::Home => detail.scroll_home(),
                    _ => {}
                }
            }
            (KeyModifiers::NONE, KeyCode::PageUp, KeyEventKind::Press) if self.tab == Tab::Jobs => {
                self.job_table.scroll_payload_up();
            }
            (KeyModifiers::NONE, KeyCode::PageDown, KeyEventKind::Press)
                if self.tab == Tab::Jobs =>
            {
                self.job_table.scroll_payload_down();
            }
            (KeyModifiers::NONE, key, KeyEventKind::Press) if self.labels_focused => match key {
                KeyCode::Esc => {
                    self.labels_focused = false;
                    self.label_filter_mut().clear();
                    self.reload();
                }
                KeyCode::Backspace => {
                    self.label_filter_mut().pop();
                }
                KeyCode::Enter => {
                    self.labels_focused = false;
                    self.reload();
                }
                KeyCode::Char(c) => {
                    self.label_filter_mut().push(c);
                }
                _ => {}
            },
            (KeyModifiers::NONE, KeyCode::Esc | KeyCode::Char('q'), KeyEventKind::Press)
            | (KeyModifiers::CONTROL, KeyCode::Char('c'), KeyEventKind::Press) => {
                self.quit();
            }
            (KeyModifiers::NONE, KeyCode::Enter, KeyEventKind::Press) => {
                self.open_detail();
            }
            (KeyModifiers::NONE, KeyCode::Char('c' | 'C'), KeyEventKind::Press) => {
                self.confirm_cancel_job();
            }
            (KeyModifiers::NONE, KeyCode::Char('s' | 'S'), KeyEventKind::Press) => {
                self.confirm_stop_schedule();
            }
            (KeyModifiers::NONE, KeyCode::Char('n' | 'N'), KeyEventKind::Press) => {
                self.open_form();
            }
            (KeyModifiers::NONE, KeyCode::Char('d' | 'D'), KeyEventKind::Press) => {
                self.duplicate();
            }
            (KeyModifiers::NONE, KeyCode::Char('r' | 'R'), KeyEventKind::Press) => {
                self.fetch(true);
            }
            (KeyModifiers::NONE, KeyCode::Char('a' | 'A'), KeyEventKind::Press) => {
                match self.tab {
                    Tab::Jobs => self.job_status = self.job_status.next(),
                    Tab::Schedules => self.schedule_status = self.schedule_status.next(),
                    Tab::Executors => return,
                }

                self.reload();
            }
            (KeyModifiers::NONE, KeyCode::Char('l' | 'L'), KeyEventKind::Press) => {
                self.labels_focused = true;
            }
            (KeyModifiers::NONE, KeyCode::Char('o' | 'O'), KeyEventKind::Press) => {
                if self.tab == Tab::Schedules {
                    self.schedule_order = match self.schedule_order {
                        ScheduleOrderBy::CreatedAtDesc => ScheduleOrderBy::CreatedAtAsc,
                        ScheduleOrderBy::CreatedAtAsc => ScheduleOrderBy::CreatedAtDesc,
                    };
                    self.reload();
                    return;
                }

                self.job_order = match self.job_order {
                    JobOrderBy::TargetExecutionTimeAsc => JobOrderBy::TargetExecutionTimeDesc,
                    JobOrderBy::TargetExecutionTimeDesc => JobOrderBy::CreatedAtAsc,
                    JobOrderBy::CreatedAtAsc => JobOrderBy::CreatedAtDesc,
                    JobOrderBy::CreatedAtDesc => JobOrderBy::TargetExecutionTimeAsc,
                };
                self.reload();
            }
            (KeyModifiers::NONE, KeyCode::Down, KeyEventKind::Press) => {
                self.move_selection(true);
            }
            (KeyModifiers::NONE, KeyCode::Up, KeyEventKind::Press) => {
                self.move_selection(false);
            }
            (KeyModifiers::NONE, KeyCode::Right, KeyEventKind::Press) => {
                self.focus_table(true);
            }
            (KeyModifiers::NONE, KeyCode::Left, KeyEventKind::Press) => {
                self.focus_table(false);
            }
            _ => {}
        }
    }

    fn fetch_executor_jobs(&mut self) {
        let Some(executor_id) = self.detail().map(|detail| detail.id.clone()) else {
            return;
        };

        self.pending
            .executor_jobs
            .start(spawn(data::update_executor_jobs(
                executor_id,
                self.client.clone(),
                self.events.sender(),
            )));
    }

    fn detail(&self) -> Option<&ui::Detail> {
        self.details[self.tab.index()].as_ref()
    }

    fn detail_mut(&mut self) -> &mut Option<ui::Detail> {
        &mut self.details[self.tab.index()]
    }

    /// Move to the next or previous tab, keeping each tab's
    /// selection, focus and detail view as they were.
    fn switch_tab(&mut self, forward: bool) {
        self.tab = if forward {
            self.tab.next()
        } else {
            self.tab.previous()
        };

        self.status.error = None;
        self.sync_focus();
        // Switching tabs does not change what was asked for, so a
        // request already in flight is left to answer.
        self.fetch(false);
    }

    /// Issue the requests the active tab needs.
    ///
    /// Unless `force` is set, a kind that already has a request in
    /// flight is skipped so that a slow server does not accumulate
    /// a backlog of identical polls.
    fn fetch(&mut self, force: bool) {
        // No filter applies to the job types, so nothing that changes
        // one is a reason to ask for them again.
        if self.pending.job_types.idle() {
            self.pending.job_types.start(spawn(data::update_job_types(
                self.client.clone(),
                self.events.sender(),
            )));
        }

        match (self.tab, self.selected_job_type()) {
            (Tab::Jobs, Some(job_type_id)) => {
                // Refreshing on the timer would drop every page loaded
                // past the first, so once one is loaded the table is
                // only refreshed when asked for.
                if force || (self.pending.jobs.idle() && self.job_table.pages <= AUTO_PAGES) {
                    let token = self.pending.jobs.next_token();
                    self.pending.jobs.start(spawn(data::update_jobs(
                        token,
                        job_type_id,
                        self.job_order,
                        self.job_status.statuses(),
                        self.client.clone(),
                        self.events.sender(),
                        self.label_filter(Tab::Jobs).to_string(),
                        None,
                    )));
                }
            }
            (Tab::Schedules, Some(job_type_id)) => {
                if force
                    || (self.pending.schedules.idle() && self.schedule_table.pages <= AUTO_PAGES)
                {
                    let token = self.pending.schedules.next_token();
                    self.pending.schedules.start(spawn(data::update_schedules(
                        token,
                        job_type_id,
                        self.schedule_order,
                        self.schedule_status.statuses(),
                        self.client.clone(),
                        self.events.sender(),
                        self.label_filter(Tab::Schedules).to_string(),
                        None,
                    )));
                }
            }
            (Tab::Executors, _) => {
                if force || self.pending.executors.idle() {
                    self.pending.executors.start(spawn(data::update_executors(
                        self.client.clone(),
                        self.events.sender(),
                    )));
                }

                if self.detail().is_some() && (force || self.pending.executor_jobs.idle()) {
                    self.fetch_executor_jobs();
                }
            }
            // Without a job type there is nothing to filter by.
            (Tab::Jobs, None) => {
                self.job_table.jobs.clear();
                self.job_table.incoming.clear();
                self.job_table.next_page = None;
            }
            (Tab::Schedules, None) => {
                self.schedule_table.schedules.clear();
                self.schedule_table.incoming.clear();
                self.schedule_table.next_page = None;
            }
        }
    }

    /// Drop the rows of the active tab and fetch again.
    ///
    /// Used whenever its filters change, so that rows matching the
    /// previous ones are never shown as if they still matched.
    fn reload(&mut self) {
        self.invalidate();
        self.clear_rows(self.tab);
        self.fetch(true);
    }

    /// Drop the rows of both tabs and fetch again.
    ///
    /// The job type is the one filter they share, so a change of it
    /// leaves the tab that is not on screen stale as well.
    fn reload_job_type(&mut self) {
        self.invalidate();
        self.clear_rows(Tab::Jobs);
        self.clear_rows(Tab::Schedules);
        self.fetch(true);
    }

    /// Discard what is in flight, whichever tab asked for it: its
    /// answer is to a question that has since been replaced.
    fn invalidate(&mut self) {
        self.pending.jobs.cancel();
        self.pending.schedules.cancel();
        self.status.error = None;
    }

    fn clear_rows(&mut self, tab: Tab) {
        match tab {
            Tab::Jobs => {
                self.job_table.jobs.clear();
                self.job_table.incoming.clear();
                self.job_table.next_page = None;
                self.job_table.pages = 0;
            }
            Tab::Schedules => {
                self.schedule_table.schedules.clear();
                self.schedule_table.incoming.clear();
                self.schedule_table.next_page = None;
                self.schedule_table.pages = 0;
            }
            Tab::Executors => {}
        }
    }

    /// What a tab's rows are filtered by. Each tab keeps its own,
    /// since a filter that narrows the jobs rarely matches a schedule.
    pub(crate) fn label_filter(&self, tab: Tab) -> &str {
        &self.label_filters[tab.index()]
    }

    fn label_filter_mut(&mut self) -> &mut String {
        &mut self.label_filters[self.tab.index()]
    }

    /// The job type used to filter the jobs and schedules tabs.
    fn selected_job_type(&self) -> Option<ora::JobTypeId> {
        self.job_type_list.state.selected().and_then(|index| {
            self.job_type_list
                .job_types
                .get(index)
                .map(|jt| jt.id.clone())
        })
    }

    /// Move the selection in whichever list has focus.
    fn move_selection(&mut self, forward: bool) {
        if self.job_type_list.focused {
            let previous = self.job_type_list.state.selected();

            if forward {
                self.job_type_list.state.select_next();
            } else {
                self.job_type_list.state.select_previous();
            }

            self.job_type_selected(previous, self.job_type_list.state.selected());
            return;
        }

        let len = self.selected_table_len();
        let state = self.selected_table_state();

        if forward {
            state.select_next();
        } else {
            state.select_previous();
        }

        let row = state.selected();

        if let Some(row) = row {
            self.reach_row(row, len);
        }
    }

    /// Fetch the next page as the selection nears the end of the rows
    /// already loaded, so that scrolling down keeps working without
    /// the list having to be fetched whole up front.
    fn reach_row(&mut self, row: usize, len: usize) {
        if row + PAGE_AHEAD < len {
            return;
        }

        match self.tab {
            Tab::Jobs => self.load_more_jobs(),
            Tab::Schedules => self.load_more_schedules(),
            Tab::Executors => {}
        }
    }

    fn selected_table_len(&self) -> usize {
        match self.tab {
            Tab::Jobs => self.job_table.jobs.len(),
            Tab::Schedules => self.schedule_table.schedules.len(),
            Tab::Executors => self.executor_table.executors.len(),
        }
    }

    /// The executors tab has no job type list, so focus
    /// is always on its table.
    fn selected_table_state(&mut self) -> &mut ratatui::widgets::TableState {
        match self.tab {
            Tab::Jobs => &mut self.job_table.state,
            Tab::Schedules => &mut self.schedule_table.state,
            Tab::Executors => &mut self.executor_table.state,
        }
    }

    /// Whether focus is on the active tab's table rather than
    /// the job type list beside it.
    fn table_focused(&self) -> bool {
        match self.tab {
            Tab::Jobs => self.job_table.focused,
            Tab::Schedules => self.schedule_table.focused,
            // This tab has no job type list, so its table always has focus.
            Tab::Executors => true,
        }
    }

    fn focus_table(&mut self, focused: bool) {
        let focused = focused || self.tab == Tab::Executors;

        match self.tab {
            Tab::Jobs => self.job_table.focused = focused,
            Tab::Schedules => self.schedule_table.focused = focused,
            Tab::Executors => self.executor_table.focused = true,
        }

        self.job_type_list.focused = !focused;

        if focused && self.selected_table_state().selected().is_none() {
            self.selected_table_state().select_next();
        }
    }

    /// Point the job type list at whatever the newly active tab
    /// had focused when it was last on screen, and make sure a
    /// focused table has a selected row to act on.
    fn sync_focus(&mut self) {
        let focused = self.table_focused();
        self.job_type_list.focused = !focused;

        if focused && self.selected_table_state().selected().is_none() {
            self.selected_table_state().select_next();
        }
    }

    fn job_type_selected(&mut self, prev_index: Option<usize>, new_index: Option<usize>) {
        if prev_index == new_index {
            return;
        }

        self.job_table.state.select(None);
        self.schedule_table.state.select(None);
        self.reload_job_type();
    }

    /// Note that the server answered, whatever it was asked.
    fn answered(&mut self) {
        self.status.updated_at = Some(Instant::now());
    }

    /// Note that the rows on screen just arrived, which also settles
    /// whatever error was on the way to them.
    fn data_updated(&mut self) {
        self.status.error = None;
        self.answered();
    }

    fn open_detail(&mut self) {
        let detail = match self.tab {
            Tab::Jobs => self.job_table.selected().map(ui::Detail::from_job),
            Tab::Schedules => self
                .schedule_table
                .selected()
                .map(ui::Detail::from_schedule),
            Tab::Executors => self
                .executor_table
                .selected()
                .map(|executor| ui::Detail::from_executor(executor, &[], true)),
        };

        let opened = detail.is_some();
        *self.detail_mut() = detail;

        // The jobs an executor is running are not part of the
        // executor list response, so they are fetched on demand.
        if opened && self.tab == Tab::Executors {
            self.executor_jobs.clear();
            self.fetch_executor_jobs();
        }
    }

    /// Close the detail view of the active tab.
    ///
    /// The answer to a request for a view that has since closed is
    /// thrown away, so the wait for it has to end here.
    fn close_detail(&mut self) {
        *self.detail_mut() = None;
        self.pending.executor_jobs.cancel();
    }

    fn refresh_detail(&mut self) {
        let Some(detail) = self.detail() else {
            return;
        };

        let rebuilt = match self.tab {
            Tab::Jobs => self
                .job_table
                .jobs
                .iter()
                .find(|job| job.id == detail.id)
                .map(ui::Detail::from_job),
            Tab::Schedules => self
                .schedule_table
                .schedules
                .iter()
                .find(|schedule| schedule.id == detail.id)
                .map(ui::Detail::from_schedule),
            Tab::Executors => self
                .executor_table
                .executors
                .iter()
                .find(|executor| executor.id.to_string() == detail.id)
                .map(|executor| {
                    ui::Detail::from_executor(
                        executor,
                        &self.executor_jobs,
                        !self.pending.executor_jobs.idle(),
                    )
                }),
        };

        if let Some(mut rebuilt) = rebuilt {
            rebuilt.restore_position(detail);
            *self.detail_mut() = Some(rebuilt);
        }
    }

    /// Whether the highlighted job can be cancelled right now.
    ///
    /// Drives both the key and the footer hint so that no action is
    /// ever offered that would do nothing.
    pub(crate) fn can_cancel_job(&self) -> bool {
        // The table keeps its selection while focus is on the type
        // list, so a row being selected is not enough on its own.
        self.tab == Tab::Jobs
            && self.table_focused()
            && self.job_table.selected().is_some_and(|job| {
                job.executions.last().is_none_or(|execution| {
                    matches!(
                        execution.status(),
                        ExecutionStatus::Pending
                            | ExecutionStatus::InProgress
                            | ExecutionStatus::Unspecified
                    )
                })
            })
    }

    /// Whether the highlighted schedule can be stopped right now.
    ///
    /// A stopped schedule cannot be stopped again, the server
    /// only ever acts on active ones.
    pub(crate) fn can_stop_schedule(&self) -> bool {
        self.tab == Tab::Schedules
            && self.table_focused()
            && self
                .schedule_table
                .selected()
                .is_some_and(|schedule| schedule.status() != ProtoScheduleStatus::Stopped)
    }

    /// Fetch the page after the rows already in the jobs table.
    fn load_more_jobs(&mut self) {
        if self.job_table.next_page.is_none() || !self.pending.jobs.idle() {
            return;
        }

        let Some(job_type_id) = self.selected_job_type() else {
            return;
        };

        let token = self.pending.jobs.next_token();
        self.pending.jobs.start(spawn(data::update_jobs(
            token,
            job_type_id,
            self.job_order,
            self.job_status.statuses(),
            self.client.clone(),
            self.events.sender(),
            self.label_filter(Tab::Jobs).to_string(),
            self.job_table.next_page.clone(),
        )));
    }

    /// Fetch the page after the rows already in the schedules table.
    fn load_more_schedules(&mut self) {
        if self.schedule_table.next_page.is_none() || !self.pending.schedules.idle() {
            return;
        }

        let Some(job_type_id) = self.selected_job_type() else {
            return;
        };

        let token = self.pending.schedules.next_token();
        self.pending.schedules.start(spawn(data::update_schedules(
            token,
            job_type_id,
            self.schedule_order,
            self.schedule_status.statuses(),
            self.client.clone(),
            self.events.sender(),
            self.label_filter(Tab::Schedules).to_string(),
            self.schedule_table.next_page.clone(),
        )));
    }

    /// Whether an overlay currently owns the keyboard.
    pub(crate) fn modal_open(&self) -> bool {
        self.form.is_some() || self.confirm.is_some() || self.detail().is_some()
    }

    /// Whether the selected row can be opened as a new job or
    /// schedule of its own.
    pub(crate) fn can_duplicate(&self) -> bool {
        self.can_create()
            && self.table_focused()
            && match self.tab {
                Tab::Jobs => self.job_table.selected().is_some(),
                Tab::Schedules => self.schedule_table.selected().is_some(),
                Tab::Executors => false,
            }
    }

    pub(crate) fn can_create(&self) -> bool {
        matches!(self.tab, Tab::Jobs | Tab::Schedules) && self.selected_job_type().is_some()
    }

    fn new_form(&self) -> Option<ui::Form> {
        if !self.can_create() {
            return None;
        }

        let index = self.job_type_list.state.selected()?;
        let job_type = self.job_type_list.job_types.get(index)?;

        let kind = if self.tab == Tab::Jobs {
            ui::FormKind::Job
        } else {
            ui::FormKind::Schedule
        };

        Some(ui::Form::new(
            kind,
            job_type.id.as_str().to_string(),
            job_type.input_schema_json.as_deref(),
        ))
    }

    fn open_form(&mut self) {
        self.form = self.new_form();
    }

    /// Open a creation form filled in from the selected row.
    ///
    /// The target time is left alone: a job is duplicated to run
    /// again, not to run again at the time the first one did.
    fn duplicate(&mut self) {
        let Some(mut form) = self.new_form() else {
            return;
        };

        match self.tab {
            Tab::Jobs => {
                let Some(job) = self.job_table.selected().and_then(|job| job.job.as_ref()) else {
                    return;
                };

                fill_from_job(&mut form, job);
            }
            Tab::Schedules => {
                let Some(schedule) = self
                    .schedule_table
                    .selected()
                    .and_then(|schedule| schedule.schedule.as_ref())
                else {
                    return;
                };

                fill_from_schedule(&mut form, schedule);
            }
            Tab::Executors => return,
        }

        self.form = Some(form);
    }

    fn submit_form(&mut self) {
        let Some(form) = self.form.as_mut() else {
            return;
        };

        if form.submitting {
            return;
        }

        if let Some(error) = form.validate() {
            form.error = Some(error);
            return;
        }

        let request = match form.kind {
            ui::FormKind::Job => build_job(form).map(Created::Job),
            ui::FormKind::Schedule => build_schedule(form).map(Created::Schedule),
        };

        match request {
            Ok(created) => {
                form.error = None;

                let prompt = match &created {
                    Created::Job(_) => format!("Create job {}?", form.job_type_id),
                    Created::Schedule(_) => format!("Create schedule {}?", form.job_type_id),
                };

                self.confirm = Some(Confirm {
                    prompt,
                    action: ConfirmAction::Create(Box::new(created)),
                });
            }
            Err(error) => form.error = Some(error),
        }
    }

    fn confirm_cancel_job(&mut self) {
        if !self.can_cancel_job() {
            return;
        }

        let Some(job) = self.job_table.selected() else {
            return;
        };

        self.confirm = Some(Confirm {
            prompt: format!("Cancel job {}?", job.id),
            action: ConfirmAction::CancelJob(job.id.clone()),
        });
    }

    fn confirm_stop_schedule(&mut self) {
        if !self.can_stop_schedule() {
            return;
        }

        let Some(schedule) = self.schedule_table.selected() else {
            return;
        };

        self.confirm = Some(Confirm {
            prompt: format!("Stop schedule {} and cancel its jobs?", schedule.id),
            action: ConfirmAction::StopSchedule(schedule.id.clone()),
        });
    }

    fn run_confirmed_action(&mut self) {
        let Some(confirm) = self.confirm.take() else {
            return;
        };

        match confirm.action {
            ConfirmAction::CancelJob(job_id) => {
                spawn(data::cancel_job(
                    job_id,
                    self.client.clone(),
                    self.events.sender(),
                ));
            }
            ConfirmAction::StopSchedule(schedule_id) => {
                spawn(data::stop_schedule(
                    schedule_id,
                    true,
                    self.client.clone(),
                    self.events.sender(),
                ));
            }
            ConfirmAction::Create(created) => {
                if let Some(form) = self.form.as_mut() {
                    form.submitting = true;
                }

                match *created {
                    Created::Job(job) => {
                        spawn(data::add_job(
                            job,
                            self.client.clone(),
                            self.events.sender(),
                        ));
                    }
                    Created::Schedule(schedule) => {
                        spawn(data::add_schedule(
                            schedule,
                            self.client.clone(),
                            self.events.sender(),
                        ));
                    }
                }
            }
        }
    }

    fn quit(&mut self) {
        self.running = false;
    }
}

#[derive(Debug)]
enum Created {
    Job(ora::proto::jobs::v1::Job),
    Schedule(ora::proto::schedules::v1::Schedule),
}

/// Parse the `key=value` rows of the labels field. A row's value may
/// be empty: a label needs a key, not a value.
fn parse_labels(rows: &[(String, String)]) -> Vec<ora::proto::common::v1::Label> {
    rows.iter()
        .filter(|(key, _)| !key.trim().is_empty())
        .map(|(key, value)| ora::proto::common::v1::Label {
            key: key.trim().to_string(),
            value: value.trim().to_string(),
        })
        .collect()
}

/// The policies shared by a job and a schedule's job template.
fn policies(
    form: &ui::Form,
) -> Result<
    (
        ora::proto::jobs::v1::TimeoutPolicy,
        ora::proto::jobs::v1::RetryPolicy,
    ),
    String,
> {
    let timeout = form.option("timeout");
    let timeout = if timeout.trim().is_empty() {
        None
    } else {
        Some(
            humantime::parse_duration(timeout.trim())
                .map_err(|error| format!("invalid timeout: {error}"))?,
        )
    };

    let retries = form.option("retries");
    let retries = if retries.trim().is_empty() {
        0
    } else {
        retries
            .trim()
            .parse::<u64>()
            .map_err(|error| format!("invalid retries: {error}"))?
    };

    Ok((
        ora::proto::jobs::v1::TimeoutPolicy {
            timeout: timeout.and_then(|d| d.try_into().ok()),
            base_time: ora::proto::jobs::v1::TimeoutBaseTime::StartTime as _,
        },
        ora::proto::jobs::v1::RetryPolicy {
            retries,
            ..Default::default()
        },
    ))
}

/// Fill in the options every job and schedule shares.
fn fill_policies(
    form: &mut ui::Form,
    timeout: Option<&ora::proto::jobs::v1::TimeoutPolicy>,
    retry: Option<&ora::proto::jobs::v1::RetryPolicy>,
) {
    if let Some(timeout) = timeout
        .and_then(|policy| policy.timeout)
        .and_then(|timeout| std::time::Duration::try_from(timeout).ok())
        .filter(|timeout| !timeout.is_zero())
    {
        form.set_option("timeout", humantime::format_duration(timeout).to_string());
    }

    if let Some(retries) = retry.map(|policy| policy.retries).filter(|r| *r > 0) {
        form.set_option("retries", retries.to_string());
    }
}

fn fill_from_job(form: &mut ui::Form, job: &ora::proto::jobs::v1::Job) {
    form.prefill_input(&job.input_payload_json);
    form.set_pairs_option("labels", label_rows(&job.labels));
    fill_policies(form, job.timeout_policy.as_ref(), job.retry_policy.as_ref());
}

fn fill_from_schedule(form: &mut ui::Form, schedule: &ora::proto::schedules::v1::Schedule) {
    use ora::proto::schedules::v1::scheduling_policy::Policy;

    if let Some(job) = schedule.job_template.as_ref() {
        form.prefill_input(&job.input_payload_json);
        fill_policies(form, job.timeout_policy.as_ref(), job.retry_policy.as_ref());
    }

    form.set_pairs_option("labels", label_rows(&schedule.labels));

    match schedule.scheduling.as_ref().and_then(|s| s.policy.as_ref()) {
        Some(Policy::Cron(cron)) => {
            form.set_option("cron", cron.cron_expression.clone());
            form.set_option_bool("immediate", cron.immediate);
        }
        Some(Policy::Interval(interval)) => {
            if let Some(interval) = interval
                .interval
                .and_then(|interval| std::time::Duration::try_from(interval).ok())
            {
                form.set_option("interval", humantime::format_duration(interval).to_string());
            }

            form.set_option_bool("immediate", interval.immediate);
        }
        None => {}
    }
}

fn label_rows(labels: &[ora::proto::common::v1::Label]) -> Vec<(String, String)> {
    labels
        .iter()
        .map(|label| (label.key.clone(), label.value.clone()))
        .collect()
}

fn build_job(form: &ui::Form) -> Result<ora::proto::jobs::v1::Job, String> {
    let target = form
        .time_option("target")?
        .unwrap_or_else(jiff::Timestamp::now);

    let (timeout_policy, retry_policy) = policies(form)?;

    Ok(ora::proto::jobs::v1::Job {
        job_type_id: form.job_type_id.clone(),
        target_execution_time: Some(std::time::SystemTime::from(target).into()),
        input_payload_json: form.payload().to_string(),
        labels: parse_labels(&form.pairs_option("labels")),
        timeout_policy: Some(timeout_policy),
        retry_policy: Some(retry_policy),
    })
}

fn build_schedule(form: &ui::Form) -> Result<ora::proto::schedules::v1::Schedule, String> {
    use ora::proto::schedules::v1::{
        SchedulingPolicy, SchedulingPolicyCron, SchedulingPolicyInterval, scheduling_policy::Policy,
    };

    let cron = form.option("cron");
    let interval = form.option("interval");
    let immediate = form.option_bool("immediate");

    let policy = if !cron.trim().is_empty() {
        ui::parse_cron(&cron).map_err(|error| format!("invalid cron: {error}"))?;

        Policy::Cron(SchedulingPolicyCron {
            cron_expression: cron.trim().to_string(),
            immediate,
            ..Default::default()
        })
    } else if !interval.trim().is_empty() {
        let interval = humantime::parse_duration(interval.trim())
            .map_err(|error| format!("invalid interval: {error}"))?;

        Policy::Interval(SchedulingPolicyInterval {
            interval: interval.try_into().ok(),
            immediate,
            ..Default::default()
        })
    } else {
        return Err("a cron expression or an interval is required".to_string());
    };

    let (timeout_policy, retry_policy) = policies(form)?;

    Ok(ora::proto::schedules::v1::Schedule {
        scheduling: Some(SchedulingPolicy {
            policy: Some(policy),
        }),
        job_template: Some(ora::proto::jobs::v1::Job {
            job_type_id: form.job_type_id.clone(),
            target_execution_time: Some(std::time::SystemTime::UNIX_EPOCH.into()),
            input_payload_json: form.payload().to_string(),
            labels: vec![],
            timeout_policy: Some(timeout_policy),
            retry_policy: Some(retry_policy),
        }),
        labels: parse_labels(&form.pairs_option("labels")),
        time_range: None,
    })
}
