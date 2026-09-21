use std::cmp;

use ora::admin::executors::ExecutorInfo;
use ratatui::{
    layout::{Constraint, Layout},
    style::{
        Modifier, Style,
        palette::tailwind::{self, SLATE},
    },
    text::{Line, Text},
    widgets::{Cell, HighlightSpacing, Row, StatefulWidget, Table, TableState, Widget},
};

use crate::tui::ui::{block, empty_message, format_age};

/// An executor that has not reported in for longer than this
/// is shown as stale.
const STALE_AFTER_SECS: u64 = 60;

#[derive(Debug, Default)]
pub(crate) struct ExecutorTable {
    pub(crate) focused: bool,
    pub(crate) loading: bool,
    pub(crate) state: TableState,
    pub(crate) executors: Vec<ExecutorInfo>,
}

impl ExecutorTable {
    pub(crate) fn selected(&self) -> Option<&ExecutorInfo> {
        self.executors.get(self.state.selected()?)
    }
}

impl Widget for &mut ExecutorTable {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let inner = block("Executors", self.focused, area, buf);

        let rows = self
            .executors
            .iter()
            .map(|executor| {
                let queues = executor
                    .queues
                    .iter()
                    .map(|queue| {
                        Line::from(format!(
                            "{} {}/{}",
                            queue.job_type_id.as_str(),
                            queue.active_executions,
                            queue.max_concurrent_executions
                        ))
                    })
                    .collect::<Vec<_>>();

                let height = u16::try_from(cmp::max(1, queues.len())).unwrap_or(1);

                let stale = executor
                    .last_seen_at
                    .elapsed()
                    .is_ok_and(|elapsed| elapsed.as_secs() > STALE_AFTER_SECS);

                Row::new([
                    Cell::new(
                        executor
                            .name
                            .clone()
                            .unwrap_or_else(|| executor.id.to_string()),
                    ),
                    Cell::new(format_age(Some(executor.last_seen_at))).style(if stale {
                        Style::new().fg(tailwind::RED.c400)
                    } else {
                        Style::new().fg(tailwind::GREEN.c400)
                    }),
                    Cell::new(Text::from(queues)),
                    Cell::new(executor.id.to_string()),
                ])
                .height(height)
            })
            .collect::<Vec<_>>();

        let table = Table::new(
            rows,
            [
                Constraint::Length(38),
                Constraint::Length(12),
                Constraint::Fill(1),
                Constraint::Length(38),
            ],
        )
        .header(
            Row::new(["Name", "Last seen", "Queues (active/max)", "ID"])
                .style(Style::new().bold().fg(tailwind::GRAY.c400)),
        )
        .row_highlight_style(Style::new().bg(SLATE.c800).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ")
        .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(table, inner, buf, &mut self.state);

        if self.executors.is_empty() {
            let [_, body] =
                Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
            empty_message(self.loading, "No executors connected.", body, buf);
        }
    }
}
