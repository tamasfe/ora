use jiff::Timestamp;
use ora::{
    admin::executors::ExecutorInfo,
    proto::{
        admin::v1::{Execution, Job, Schedule, ScheduleStatus},
        common::v1::Label,
        jobs::v1::{RetryPolicy, TimeoutPolicy},
    },
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Style, palette::tailwind},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::tui::ui::{
    execution_status_label, execution_status_style, field, format_duration, format_time,
    format_time_with_age, pretty_json, schedules::policy_summary, timestamp_of,
};

/// A scrollable, paged read-only view of a single job or schedule.
#[derive(Debug)]
pub(crate) struct Detail {
    /// The ID of the job or schedule being shown,
    /// used to rebuild the view when new data arrives.
    pub(crate) id: String,
    title: String,
    pages: Vec<Page>,
    selected: usize,
    scroll: u16,
    /// The height the body was last rendered at, which is how much of
    /// a page is on screen and so how far it can be scrolled.
    height: u16,
}

#[derive(Debug)]
struct Page {
    name: &'static str,
    lines: Vec<Line<'static>>,
}

impl Detail {
    pub(crate) fn from_job(job: &Job) -> Self {
        let def = job.job.as_ref();

        let pages = vec![
            Page {
                name: "Overview",
                lines: job_overview(job),
            },
            Page {
                name: "Input",
                lines: json_lines(def.map_or("", |d| d.input_payload_json.as_str())),
            },
            Page {
                name: "Output",
                lines: job_output(job),
            },
            Page {
                name: "Executions",
                lines: job_executions(job),
            },
        ];

        Self {
            id: job.id.clone(),
            title: def.map_or_else(|| "Job".to_string(), |d| d.job_type_id.clone()),
            pages,
            selected: 0,
            scroll: 0,
            height: 0,
        }
    }

    pub(crate) fn from_schedule(schedule: &Schedule) -> Self {
        let def = schedule.schedule.as_ref();
        let template = def.and_then(|d| d.job_template.as_ref());

        let pages = vec![
            Page {
                name: "Overview",
                lines: schedule_overview(schedule),
            },
            Page {
                name: "Job template",
                lines: json_lines(template.map_or("", |t| t.input_payload_json.as_str())),
            },
        ];

        Self {
            id: schedule.id.clone(),
            title: template.map_or_else(|| "Schedule".to_string(), |t| t.job_type_id.clone()),
            pages,
            selected: 0,
            scroll: 0,
            height: 0,
        }
    }

    pub(crate) fn from_executor(executor: &ExecutorInfo, jobs: &[Job], loading: bool) -> Self {
        let pages = vec![
            Page {
                name: "Overview",
                lines: executor_overview(executor),
            },
            Page {
                name: "Recent jobs",
                lines: executor_jobs(jobs, loading),
            },
        ];

        Self {
            id: executor.id.to_string(),
            title: executor
                .name
                .clone()
                .unwrap_or_else(|| executor.id.to_string()),
            pages,
            selected: 0,
            scroll: 0,
            height: 0,
        }
    }

    /// Carry the page and scroll position over from a previous
    /// version of this view after the data was refreshed.
    pub(crate) fn restore_position(&mut self, previous: &Detail) {
        self.selected = previous.selected.min(self.pages.len().saturating_sub(1));
        self.height = previous.height;
        self.scroll = previous.scroll.min(self.max_scroll());
    }

    pub(crate) fn select_next_page(&mut self) {
        self.selected = (self.selected + 1) % self.pages.len();
        self.scroll = 0;
    }

    pub(crate) fn select_previous_page(&mut self) {
        self.selected = self.selected.checked_sub(1).unwrap_or(self.pages.len() - 1);
        self.scroll = 0;
    }

    pub(crate) fn scroll_up(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub(crate) fn scroll_down(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_add(amount).min(self.max_scroll());
    }

    pub(crate) fn scroll_home(&mut self) {
        self.scroll = 0;
    }

    /// How far the page can be scrolled before its last line is at
    /// the bottom of the body. Zero while the page fits.
    fn max_scroll(&self) -> u16 {
        u16::try_from(self.pages[self.selected].lines.len())
            .unwrap_or(u16::MAX)
            .saturating_sub(self.height)
    }
}

impl Widget for &mut Detail {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        Clear.render(area, buf);

        let block = Block::new()
            .title(format!(" {} ", self.title))
            .title_style(Style::new().bold().fg(tailwind::BLUE.c400))
            .borders(Borders::all())
            .border_style(Style::new().fg(tailwind::BLUE.c400))
            .padding(ratatui::widgets::Padding::horizontal(1));

        let inner = block.inner(area);
        block.render(area, buf);

        let [tabs, body] =
            Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(inner);

        let mut tab_line = Line::default();
        for (index, page) in self.pages.iter().enumerate() {
            tab_line.push_span(Span::from(format!(" {} ", page.name)).style(
                if index == self.selected {
                    Style::new().bold().fg(tailwind::ORANGE.c400)
                } else {
                    Style::new().fg(tailwind::GRAY.c500)
                },
            ));
        }
        tab_line.push_span(
            Span::from("  ←/→ page  ↑/↓ scroll  esc close")
                .style(Style::new().fg(tailwind::GRAY.c600)),
        );
        tab_line.render(tabs, buf);

        self.height = body.height;
        self.scroll = self.scroll.min(self.max_scroll());

        Paragraph::new(self.pages[self.selected].lines.clone())
            .scroll((self.scroll, 0))
            .render(body, buf);
    }
}

fn json_lines(json: &str) -> Vec<Line<'static>> {
    if json.trim().is_empty() {
        return vec![Line::from("(empty)").style(Style::new().fg(tailwind::GRAY.c500))];
    }

    pretty_json(json).lines().map(text_line).collect()
}

fn text_line(line: &str) -> Line<'static> {
    Line::from(line.to_string())
}

fn job_overview(job: &Job) -> Vec<Line<'static>> {
    let mut lines = vec![field("ID", job.id.clone())];

    if let Some(def) = job.job.as_ref() {
        lines.push(field("Type", def.job_type_id.clone()));
        lines.push(field("Target", format_time(def.target_execution_time)));
        lines.extend(timeout_retry_lines(
            def.timeout_policy.as_ref(),
            def.retry_policy.as_ref(),
        ));
    }

    lines.push(field("Created", format_time_with_age(job.created_at)));

    if let Some(schedule_id) = job.schedule_id.as_ref() {
        lines.push(field("Schedule", schedule_id.clone()));
    }

    if let Some(execution) = job.executions.last() {
        let status = execution.status();
        lines.push(Line::from(vec![
            Span::from("Status: ").style(Style::new().bold()),
            Span::from(execution_status_label(status)).style(execution_status_style(status)),
        ]));
    }

    lines.push(field("Attempts", job.executions.len().to_string()));

    if let Some(def) = job.job.as_ref() {
        push_labels(&mut lines, &def.labels);
    }

    lines
}

fn job_output(job: &Job) -> Vec<Line<'static>> {
    let Some(execution) = job.executions.last() else {
        return vec![Line::from("(no executions)").style(Style::new().fg(tailwind::GRAY.c500))];
    };

    if let Some(reason) = execution.failure_reason.as_ref() {
        return reason
            .lines()
            .map(|line| text_line(line).style(Style::new().fg(tailwind::RED.c400)))
            .collect();
    }

    json_lines(execution.output_json.as_deref().unwrap_or(""))
}

fn job_executions(job: &Job) -> Vec<Line<'static>> {
    if job.executions.is_empty() {
        return vec![Line::from("(no executions)").style(Style::new().fg(tailwind::GRAY.c500))];
    }

    let mut lines = Vec::new();

    for (index, execution) in job.executions.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }

        let status = execution.status();

        lines.push(Line::from(vec![
            Span::from(format!("#{} ", index + 1)).style(Style::new().bold()),
            Span::from(execution_status_label(status)).style(execution_status_style(status)),
        ]));

        lines.push(text_line(&format!(
            "  target    {}",
            format_time(execution.target_execution_time)
        )));
        lines.push(text_line(&format!(
            "  started   {}",
            format_time(execution.started_at)
        )));
        lines.push(text_line(&format!(
            "  ended     {}",
            format_time(ended_at(execution))
        )));

        if let Some(duration) = execution_duration(execution) {
            lines.push(text_line(&format!("  duration  {duration}")));
        }

        if let Some(executor_id) = execution.executor_id.as_ref() {
            lines.push(text_line(&format!("  executor  {executor_id}")));
        }

        if let Some(reason) = execution.failure_reason.as_ref() {
            lines
                .push(Line::from(format!("  {reason}")).style(Style::new().fg(tailwind::RED.c400)));
        }
    }

    lines
}

fn ended_at(execution: &Execution) -> Option<Timestamp> {
    timestamp_of(
        execution
            .succeeded_at
            .or(execution.failed_at)
            .or(execution.cancelled_at)?,
    )
}

fn execution_duration(execution: &Execution) -> Option<String> {
    let started = timestamp_of(execution.started_at?)?;
    Some(format!(
        "{:#}",
        ended_at(execution)?.duration_since(started)
    ))
}

fn schedule_overview(schedule: &Schedule) -> Vec<Line<'static>> {
    let mut lines = vec![field("ID", schedule.id.clone())];

    let status = schedule.status();
    lines.push(Line::from(vec![
        Span::from("Status: ").style(Style::new().bold()),
        Span::from(match status {
            ScheduleStatus::Stopped => "stopped",
            _ => "active",
        })
        .style(match status {
            ScheduleStatus::Stopped => Style::new().fg(tailwind::GRAY.c400),
            _ => Style::new().fg(tailwind::GREEN.c400),
        }),
    ]));

    if let Some(def) = schedule.schedule.as_ref() {
        if let Some(template) = def.job_template.as_ref() {
            lines.push(field("Type", template.job_type_id.clone()));
        }

        lines.push(field("Policy", policy_summary(def.scheduling.as_ref())));

        if let Some(range) = def.time_range.as_ref() {
            if range.start.is_some() {
                lines.push(field("Starts", format_time(range.start)));
            }
            if range.end.is_some() {
                lines.push(field("Ends", format_time(range.end)));
            }
        }

        if let Some(template) = def.job_template.as_ref() {
            lines.extend(timeout_retry_lines(
                template.timeout_policy.as_ref(),
                template.retry_policy.as_ref(),
            ));
        }
    }

    lines.push(field("Created", format_time_with_age(schedule.created_at)));

    if schedule.stopped_at.is_some() {
        lines.push(field("Stopped", format_time(schedule.stopped_at)));
    }

    if let Some(def) = schedule.schedule.as_ref() {
        push_labels(&mut lines, &def.labels);
    }

    lines
}

/// The "Timeout" and "Retries" fields shared by a job and a
/// schedule's job template.
fn timeout_retry_lines(
    timeout_policy: Option<&TimeoutPolicy>,
    retry_policy: Option<&RetryPolicy>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if let Some(timeout) = timeout_policy.and_then(|policy| policy.timeout) {
        lines.push(field("Timeout", format_duration(Some(timeout))));
    }

    if let Some(retry) = retry_policy {
        lines.push(field(
            "Retries",
            format!(
                "{} ({}, backoff {})",
                retry.retries,
                retry.backoff_strategy().as_str_name().to_lowercase(),
                format_duration(retry.backoff_duration)
            ),
        ));
    }

    lines
}

/// A blank line, a "Labels" heading and one line per label,
/// appended only when there are any.
fn push_labels(lines: &mut Vec<Line<'static>>, labels: &[Label]) {
    if labels.is_empty() {
        return;
    }

    lines.push(Line::default());
    lines.push(Line::from("Labels").style(Style::new().bold()));

    for label in labels {
        lines.push(text_line(&format!("  {}={}", label.key, label.value)));
    }
}

fn executor_overview(executor: &ExecutorInfo) -> Vec<Line<'static>> {
    let mut lines = vec![field("ID", executor.id.to_string())];

    if let Some(name) = executor.name.as_ref() {
        lines.push(field("Name", name.clone()));
    }

    lines.push(field(
        "Last seen",
        format_time_with_age(Some(executor.last_seen_at)),
    ));

    let active: u64 = executor.queues.iter().map(|q| q.active_executions).sum();
    let capacity: u64 = executor
        .queues
        .iter()
        .map(|q| q.max_concurrent_executions)
        .sum();

    lines.push(field("Load", format!("{active}/{capacity}")));

    if !executor.queues.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from("Queues").style(Style::new().bold()));

        for queue in &executor.queues {
            lines.push(text_line(&format!(
                "  {} {}/{}",
                queue.job_type_id.as_str(),
                queue.active_executions,
                queue.max_concurrent_executions
            )));
        }
    }

    lines
}

/// The name of the first column, which is also its narrowest width.
const JOB_TYPE: &str = "Job type";

fn executor_jobs(jobs: &[Job], loading: bool) -> Vec<Line<'static>> {
    if jobs.is_empty() {
        // Fetched only once the view is opened, so empty means nothing yet.
        let message = if loading { "Loading…" } else { "(no jobs)" };

        return vec![Line::from(message).style(Style::new().fg(tailwind::GRAY.c500))];
    }

    let width = jobs
        .iter()
        .filter_map(|job| job.job.as_ref())
        .map(|def| def.job_type_id.chars().count())
        .chain(std::iter::once(JOB_TYPE.len()))
        .max()
        .unwrap_or(0);

    let header = Line::from(format!(
        "{JOB_TYPE:<width$} {:<12}{:<30}ID",
        "Status", "Created"
    ))
    .style(Style::new().bold().fg(tailwind::GRAY.c400));

    std::iter::once(header)
        .chain(jobs.iter().map(|job| {
            let status = job.executions.last().map(Execution::status);
            let job_type = job
                .job
                .as_ref()
                .map_or("", |def| def.job_type_id.as_str())
                .to_string();

            let mut spans = vec![Span::from(format!("{job_type:<width$} "))];

            match status {
                Some(status) => spans.push(
                    Span::from(format!("{:<12}", execution_status_label(status)))
                        .style(execution_status_style(status)),
                ),
                None => spans.push(Span::from(format!("{:<12}", ""))),
            }

            // Padded so the IDs line up to be copied out.
            spans.push(Span::from(format!(
                "{:<30}",
                format_time_with_age(job.created_at)
            )));
            spans.push(Span::from(job.id.clone()));

            Line::from(spans)
        }))
        .collect()
}
