use std::time::SystemTime;

use jiff::{Timestamp, TimestampRound};
use ora::proto::admin::v1::{Execution, Job};
use ratatui::{
    layout::{Constraint, Layout},
    style::{
        Modifier, Style,
        palette::tailwind::{self, SLATE},
    },
    symbols,
    widgets::{
        Block, Borders, Cell, HighlightSpacing, Padding, Paragraph, Row, StatefulWidget, Table,
        TableState, Widget, Wrap,
    },
};

use crate::tui::ui::{
    compact_duration, empty_message, execution_status_label, execution_status_style, field,
    format_time, format_time_with_age, pretty_json, timestamp_of,
};

/// How many lines the input/output payloads scroll per key press.
const PAYLOAD_SCROLL_STEP: u16 = 3;

#[derive(Debug, Default)]
pub(crate) struct JobTable {
    pub(crate) focused: bool,
    pub(crate) loading: bool,
    pub(crate) state: TableState,
    pub(crate) jobs: Vec<Job>,
    /// The token for the page after the rows held here, when the
    /// server said there is one.
    pub(crate) next_page: Option<String>,
    /// How many pages of rows have arrived.
    pub(crate) pages: usize,
    /// The pages of the fetch in progress. The rows on screen are
    /// replaced from here once enough of them have arrived.
    pub(crate) incoming: Vec<Job>,
    /// How far the input/output payloads are scrolled, and the job
    /// they belong to, so selecting a different one resets it.
    payload_scroll: u16,
    payload_job_id: Option<String>,
}

impl JobTable {
    pub(crate) fn selected(&self) -> Option<&Job> {
        self.jobs.get(self.state.selected()?)
    }

    pub(crate) fn scroll_payload_up(&mut self) {
        self.payload_scroll = self.payload_scroll.saturating_sub(PAYLOAD_SCROLL_STEP);
    }

    pub(crate) fn scroll_payload_down(&mut self) {
        self.payload_scroll = self.payload_scroll.saturating_add(PAYLOAD_SCROLL_STEP);
    }
}

impl Widget for &mut JobTable {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let layout = Layout::horizontal([Constraint::Length(44), Constraint::Fill(1)]);
        let [left, right] = layout.areas(area);

        let block = Block::new()
            .title(" Jobs ")
            .title_style(Style::new().bold())
            .borders(Borders::all())
            .border_set(symbols::border::PLAIN)
            .border_style(if self.focused {
                Style::new().fg(tailwind::BLUE.c400)
            } else {
                Style::default()
            });

        let block_inner = block.inner(left);

        let rows = self
            .jobs
            .iter()
            .map(|job| {
                // Every job gets a row, including one with no execution yet,
                // so the highlighted line is always the job an action hits.
                let target_time = job
                    .job
                    .as_ref()
                    .and_then(|def| def.target_execution_time)
                    .and_then(|ts| Timestamp::try_from(SystemTime::try_from(ts).ok()?).ok())
                    .and_then(|ts| {
                        ts.round(TimestampRound::new().smallest(jiff::Unit::Second))
                            .ok()
                    })
                    .map(|ts| ts.strftime("%Y-%m-%d %H:%M:%S%:z").to_string())
                    .unwrap_or_default();

                let status = job.executions.last().map(Execution::status);

                Row::new([
                    Cell::new(target_time),
                    match status {
                        Some(status) => Cell::new(execution_status_label(status))
                            .style(execution_status_style(status)),
                        None => Cell::new(""),
                    },
                    Cell::new(job.id.clone()),
                ])
            })
            .collect::<Vec<_>>();

        let table = Table::new(rows, [Constraint::Length(26), Constraint::Length(12)])
            .block(block)
            .row_highlight_style(Style::new().bg(SLATE.c800).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ")
            .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(table, left, buf, &mut self.state);

        if self.jobs.is_empty() {
            empty_message(
                self.loading,
                "No jobs match the current filter.",
                block_inner,
                buf,
            );
        }

        let selected = self.state.selected().and_then(|i| self.jobs.get(i));

        if selected.map(|job| job.id.as_str()) != self.payload_job_id.as_deref() {
            self.payload_scroll = 0;
            self.payload_job_id = selected.map(|job| job.id.clone());
        }

        JobDetails(selected, self.payload_scroll).render(right, buf);
    }
}

#[derive(Debug, Default)]
pub(crate) struct JobDetails<'a>(Option<&'a Job>, u16);

impl Widget for JobDetails<'_> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let block = Block::new()
            .title(" Details ")
            .title_style(Style::new().bold())
            .borders(Borders::all())
            .border_set(symbols::border::PLAIN);

        let inner = block.inner(area);
        block.render(area, buf);

        let Some(job) = self.0 else {
            return;
        };

        let Some(def) = job.job.as_ref() else {
            return;
        };

        let mut meta = vec![
            field("ID", job.id.clone()),
            field("Created", format_time_with_age(job.created_at)),
        ];

        meta.push(
            match job
                .executions
                .last()
                .and_then(|exec| job_state_field(job, exec))
            {
                Some((label, text)) => field(label, text),
                None => field("Target", format_time_with_age(def.target_execution_time)),
            },
        );

        if !def.labels.is_empty() {
            meta.push(field(
                "Labels",
                def.labels
                    .iter()
                    .map(|label| format!("{}={}", label.key, label.value))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }

        let meta_height = u16::try_from(meta.len()).unwrap_or(4) + 1;

        let [meta_area, payloads] =
            Layout::vertical([Constraint::Length(meta_height), Constraint::Fill(1)])
                .horizontal_margin(1)
                .areas(inner);

        Paragraph::new(meta).render(meta_area, buf);

        // Stacked rather than side by side: the pane is far wider than tall.
        let [input, output] =
            Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(payloads);

        let scroll = self.1;

        payload(
            "Input",
            &pretty_json(&def.input_payload_json),
            None,
            scroll,
            input,
            buf,
        );

        match job.executions.last() {
            Some(exec) => match (exec.failure_reason.as_ref(), exec.output_json.as_ref()) {
                (Some(reason), _) => payload(
                    "Output",
                    reason,
                    Some(Style::new().fg(tailwind::RED.c400)),
                    scroll,
                    output,
                    buf,
                ),
                (_, Some(json)) => {
                    payload("Output", &pretty_json(json), None, scroll, output, buf);
                }
                _ => payload("Output", "", None, scroll, output, buf),
            },
            None => payload("Output", "", None, scroll, output, buf),
        }
    }
}

/// A bordered, wrapping, scrollable pane for a JSON payload or
/// failure reason.
fn payload(
    title: &str,
    text: &str,
    style: Option<Style>,
    scroll: u16,
    area: ratatui::prelude::Rect,
    buf: &mut ratatui::prelude::Buffer,
) {
    Paragraph::new(text.to_string())
        .style(style.unwrap_or_default())
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(
            Block::new()
                .title(format!(" {title} "))
                .borders(Borders::all())
                .border_set(symbols::border::PLAIN)
                .padding(Padding::horizontal(1)),
        )
        .render(area, buf);
}

/// The one time field that follows "Created", in place of "Target",
/// once the job is past being merely pending: when it started while
/// running, or how it ended once it has. `None` leaves the caller to
/// show the target instead.
fn job_state_field(job: &Job, exec: &Execution) -> Option<(&'static str, String)> {
    if let Some(cancelled) = exec.cancelled_at {
        return Some(("Cancelled", format_time_with_age(Some(cancelled))));
    }

    if let Some(ended) = exec.succeeded_at.or(exec.failed_at) {
        let ended = timestamp_of(ended)?;

        let Some(target) = job
            .job
            .as_ref()
            .and_then(|def| def.target_execution_time)
            .and_then(timestamp_of)
        else {
            return Some(("Finished", format_time(Some(ended))));
        };

        let duration = ended.duration_since(target).abs();

        return Some((
            "Finished",
            format!(
                "{} ({})",
                format_time(Some(ended)),
                compact_duration(duration)
            ),
        ));
    }

    exec.started_at
        .map(|started| ("Started", format_time_with_age(Some(started))))
}
