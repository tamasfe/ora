use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ora::{AdminClient, JobOrderBy};
use ratatui::DefaultTerminal;
use tokio::spawn;

use crate::tui::{
    events::{AppEvent, Events},
    ui::JobTypeList,
};

mod data;
mod events;
mod ui;

pub(crate) async fn run(client: AdminClient) -> eyre::Result<()> {
    let terminal = ratatui::init();
    let result = App::new(client).run(terminal).await;
    ratatui::restore();
    result
}

#[derive(Debug)]
pub struct App {
    client: AdminClient,
    running: bool,
    events: Events,
    job_type_list: ui::JobTypeList,
    job_table: ui::JobTable,
    job_order: JobOrderBy,
    active_only: bool,
    labels_focused: bool,
    label_filter: String,
}

impl App {
    /// Construct a new instance of [`App`].
    pub fn new(client: AdminClient) -> Self {
        Self {
            client,
            running: false,
            job_type_list: JobTypeList::default(),
            job_table: ui::JobTable::default(),
            events: Events::default(),
            job_order: JobOrderBy::CreatedAtDesc,
            active_only: false,
            labels_focused: false,
            label_filter: String::new(),
        }
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> eyre::Result<()> {
        spawn({
            let events = self.events.sender();

            async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    let _ = events.send_async(AppEvent::Refresh).await;
                }
            }
        });

        spawn(data::update_job_types(
            self.client.clone(),
            self.events.sender(),
        ));

        self.job_type_list.focused = true;
        self.running = true;
        while self.running {
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            self.handle_events().await?;
        }
        Ok(())
    }

    async fn handle_events(&mut self) -> eyre::Result<()> {
        if let Some(event) = self.events.next().await {
            match event {
                AppEvent::Term(event) => match event {
                    Event::Key(key_event) => self.on_key(key_event),
                    _ => {}
                },
                AppEvent::JobTypesUpdated(job_type_infos) => {
                    self.job_type_list.job_types = job_type_infos;

                    let prev_selection = self.job_type_list.state.selected();

                    if prev_selection.is_none() {
                        self.job_type_list.state.select_next();
                        self.job_type_selected(prev_selection, self.job_type_list.state.selected());
                    }
                }
                AppEvent::JobsUpdated(jobs) => {
                    self.job_table.jobs = jobs;
                }
                AppEvent::Refresh => {
                    spawn(data::update_job_types(
                        self.client.clone(),
                        self.events.sender(),
                    ));

                    let job_type_id = self.job_type_list.state.selected().and_then(|index| {
                        self.job_type_list
                            .job_types
                            .get(index)
                            .map(|jt| jt.id.clone())
                    });

                    spawn(data::update_jobs(
                        job_type_id,
                        self.job_order,
                        self.active_only,
                        self.client.clone(),
                        self.events.sender(),
                        self.label_filter.clone(),
                    ));
                }
            }
        }

        Ok(())
    }

    fn on_key(&mut self, event: KeyEvent) {
        match (event.modifiers, event.code, event.kind) {
            (KeyModifiers::NONE, key, KeyEventKind::Press) if self.labels_focused => match key {
                KeyCode::Esc => {
                    self.labels_focused = false;
                    self.label_filter.clear();
                    _ = self.events.sender().send(AppEvent::Refresh);
                }
                KeyCode::Backspace => {
                    self.label_filter.pop();
                }
                KeyCode::Enter => {
                    self.labels_focused = false;
                    _ = self.events.sender().send(AppEvent::Refresh);
                }
                KeyCode::Char(c) => {
                    self.label_filter.push(c);
                }
                _ => {}
            },
            (KeyModifiers::NONE, KeyCode::Esc | KeyCode::Char('q'), KeyEventKind::Press)
            | (KeyModifiers::CONTROL, KeyCode::Char('c'), KeyEventKind::Press) => {
                self.quit();
            }
            (KeyModifiers::NONE, KeyCode::Char('r' | 'R'), KeyEventKind::Press) => {
                _ = self.events.sender().send(AppEvent::Refresh);
            }
            (KeyModifiers::NONE, KeyCode::Char('a' | 'A'), KeyEventKind::Press) => {
                self.active_only = !self.active_only;
                _ = self.events.sender().send(AppEvent::Refresh);
            }
            (KeyModifiers::NONE, KeyCode::Char('l' | 'L'), KeyEventKind::Press) => {
                self.labels_focused = true;
            }
            (KeyModifiers::NONE, KeyCode::Char('o' | 'O'), KeyEventKind::Press) => {
                self.job_order = match self.job_order {
                    JobOrderBy::TargetExecutionTimeAsc => JobOrderBy::TargetExecutionTimeDesc,
                    JobOrderBy::TargetExecutionTimeDesc => JobOrderBy::CreatedAtAsc,
                    JobOrderBy::CreatedAtAsc => JobOrderBy::CreatedAtDesc,
                    JobOrderBy::CreatedAtDesc => JobOrderBy::TargetExecutionTimeAsc,
                };
                _ = self.events.sender().send(AppEvent::Refresh);
            }
            (KeyModifiers::NONE, KeyCode::Down, KeyEventKind::Press) => {
                if self.job_type_list.focused {
                    let prev_index = self.job_type_list.state.selected();
                    self.job_type_list.state.select_next();
                    self.job_type_selected(prev_index, self.job_type_list.state.selected());
                }

                if self.job_table.focused {
                    self.job_table.state.select_next();
                }
            }
            (KeyModifiers::NONE, KeyCode::Up, KeyEventKind::Press) => {
                if self.job_type_list.focused {
                    let prev_index = self.job_type_list.state.selected();
                    self.job_type_list.state.select_previous();
                    self.job_type_selected(prev_index, self.job_type_list.state.selected());
                }

                if self.job_table.focused {
                    self.job_table.state.select_previous();
                }
            }
            (KeyModifiers::NONE, KeyCode::Right, KeyEventKind::Press) => {
                self.job_type_list.focused = false;
                self.job_table.focused = true;

                if self.job_table.state.selected().is_none() {
                    self.job_table.state.select_next();
                }
            }
            (KeyModifiers::NONE, KeyCode::Left, KeyEventKind::Press) => {
                self.job_type_list.focused = true;
                self.job_table.focused = false;
            }
            _ => {}
        }
    }

    fn job_type_selected(&mut self, prev_index: Option<usize>, new_index: Option<usize>) {
        if prev_index == new_index {
            return;
        }

        self.job_table.state.select(None);
        _ = self.events.sender().send(AppEvent::Refresh);
    }

    fn quit(&mut self) {
        self.running = false;
    }
}
