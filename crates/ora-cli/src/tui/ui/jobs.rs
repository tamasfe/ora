use std::time::SystemTime;

use jiff::{SignedDurationRound, Timestamp, TimestampRound};
use ora::proto::admin::v1::Job;
use ratatui::{
    layout::{Constraint, Layout},
    style::{
        Modifier, Style,
        palette::tailwind::{self, SLATE},
    },
    symbols,
    text::Line,
    widgets::{
        Block, Borders, Cell, HighlightSpacing, Paragraph, Row, StatefulWidget, Table, TableState,
        Widget,
    },
};
use serde_json::Value;

#[derive(Debug, Default)]
pub(crate) struct JobTable {
    pub(crate) focused: bool,
    pub(crate) state: TableState,
    pub(crate) jobs: Vec<Job>,
}

impl Widget for &mut JobTable {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let layout = Layout::horizontal([Constraint::Length(36), Constraint::Fill(1)]);
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

        let rows = self
            .jobs
            .iter()
            .filter_map(|job| {
                let def = job.job.as_ref()?;

                let target_time =
                    Timestamp::try_from(SystemTime::try_from(def.target_execution_time?).ok()?)
                        .ok()?
                        .round(TimestampRound::new().smallest(jiff::Unit::Second))
                        .ok()?;

                let status = match job.executions.last()?.status() {
                    ora::proto::admin::v1::ExecutionStatus::Unspecified
                    | ora::proto::admin::v1::ExecutionStatus::Pending => "pending",
                    ora::proto::admin::v1::ExecutionStatus::InProgress => "in-progress",
                    ora::proto::admin::v1::ExecutionStatus::Succeeded => "succeeded",
                    ora::proto::admin::v1::ExecutionStatus::Failed => "failed",
                    ora::proto::admin::v1::ExecutionStatus::Cancelled => "cancelled",
                };

                Some(Row::new([
                    Cell::new(target_time.to_string()),
                    Cell::new(status).style(match status {
                        "succeeded" => Style::new().fg(tailwind::GREEN.c400),
                        "failed" | "cancelled" => Style::new().fg(tailwind::RED.c400),
                        "in-progress" => Style::new().fg(tailwind::YELLOW.c400),
                        _ => Style::new().fg(tailwind::GRAY.c400),
                    }),
                    Cell::new(job.id.clone()),
                ]))
            })
            .collect::<Vec<_>>();

        let table = Table::new(rows, [Constraint::Length(20), Constraint::Length(10)])
            .block(block)
            .row_highlight_style(Style::new().bg(SLATE.c800).add_modifier(Modifier::BOLD))
            .highlight_symbol("> ")
            .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(table, left, buf, &mut self.state);
        JobDetails(self.state.selected().and_then(|i| self.jobs.get(i))).render(right, buf);
    }
}

#[derive(Debug, Default)]
pub(crate) struct JobDetails<'a>(Option<&'a Job>);

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

        block.render(area, buf);

        let Some(job) = self.0 else {
            return;
        };

        let Some(def) = job.job.as_ref() else {
            return;
        };

        let layout = Layout::horizontal([
            Constraint::Length(38),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ])
        .margin(1);
        let [meta, input, output] = layout.areas(area);

        let meta_layout = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Fill(1),
        ])
        .margin(1);

        let [id_area, created_area, finished_area, labels_area] = meta_layout.areas(meta);

        Paragraph::new(vec![Line::from(job.id.as_str())])
            .block(
                Block::new()
                    .title("ID")
                    .title_style(Style::new().bold())
                    .borders(Borders::NONE),
            )
            .render(id_area, buf);

        let created_at = job
            .created_at
            .and_then(|ts| Timestamp::try_from(SystemTime::try_from(ts).ok()?).ok())
            .and_then(|ts| {
                ts.round(TimestampRound::new().smallest(jiff::Unit::Second))
                    .ok()
            })
            .unwrap_or_default();

        Paragraph::new(Line::from(created_at.to_string()))
            .block(
                Block::new()
                    .title("Created")
                    .title_style(Style::new().bold())
                    .borders(Borders::NONE),
            )
            .render(created_area, buf);

        let target_time = job
            .job
            .as_ref()
            .and_then(|def| {
                Timestamp::try_from(SystemTime::try_from(def.target_execution_time?).ok()?).ok()
            })
            .unwrap_or_default();

        let finished_at = job
            .executions
            .last()
            .and_then(|exec| exec.succeeded_at.or(exec.failed_at).or(exec.cancelled_at))
            .and_then(|ts| Timestamp::try_from(SystemTime::try_from(ts).ok()?).ok());

        let finished_at_text = match finished_at {
            Some(ts) => {
                let duration = ts.duration_since(target_time);

                let duration = duration
                    .round(SignedDurationRound::new().smallest(jiff::Unit::Millisecond))
                    .unwrap_or(duration);

                let ts = ts
                    .round(TimestampRound::new().smallest(jiff::Unit::Second))
                    .unwrap_or(ts);

                format!("{ts} ({duration:#})")
            }
            None => String::new(),
        };

        Paragraph::new(Line::from(finished_at_text))
            .block(
                Block::new()
                    .title("Finished")
                    .title_style(Style::new().bold())
                    .borders(Borders::NONE),
            )
            .render(finished_area, buf);

        let mut labels = Vec::new();

        for label in &def.labels {
            labels.push(Line::from(format!("{}={}", label.key, label.value)));
        }

        Paragraph::new(labels)
            .block(
                Block::new()
                    .title("Labels")
                    .title_style(Style::new().bold())
                    .borders(Borders::NONE),
            )
            .render(labels_area, buf);

        Paragraph::new(
            serde_json::to_string_pretty(
                &serde_json::from_str::<Value>(&def.input_payload_json).unwrap_or_default(),
            )
            .unwrap_or_default(),
        )
        .block(
            Block::new()
                .title(" Input ")
                .borders(Borders::all())
                .border_set(symbols::border::PLAIN),
        )
        .render(input, buf);

        Paragraph::new(match job.executions.last() {
            Some(exec) => match (exec.failure_reason.as_ref(), exec.output_json.as_ref()) {
                (Some(reason), _) => reason.clone(),
                (_, Some(output)) => serde_json::to_string_pretty(
                    &serde_json::from_str::<Value>(output).unwrap_or_default(),
                )
                .unwrap_or_default(),
                _ => String::new(),
            },
            None => String::new(),
        })
        .block(
            Block::new()
                .title(" Output ")
                .borders(Borders::all())
                .border_set(symbols::border::PLAIN),
        )
        .render(output, buf);
    }
}
