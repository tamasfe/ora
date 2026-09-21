use jiff::Timestamp;
use ora::proto::{
    admin::v1::{Schedule, ScheduleStatus},
    schedules::v1::{SchedulingPolicy, scheduling_policy::Policy},
};
use ratatui::{
    layout::{Constraint, Layout},
    style::{
        Modifier, Style,
        palette::tailwind::{self, SLATE},
    },
    text::{Line, Text},
    widgets::{Cell, HighlightSpacing, Row, StatefulWidget, Table, TableState, Widget},
};

use crate::tui::ui::{
    TIME_FORMAT, block, compact_duration, empty_message, format_age, format_duration,
};

/// The narrowest and widest the policy column is allowed to be.
const POLICY_WIDTH: std::ops::RangeInclusive<usize> = 16..=60;

#[derive(Debug, Default)]
pub(crate) struct ScheduleTable {
    pub(crate) focused: bool,
    pub(crate) loading: bool,
    pub(crate) state: TableState,
    pub(crate) schedules: Vec<Schedule>,
    /// The token for the page after the rows held here, when the
    /// server said there is one.
    pub(crate) next_page: Option<String>,
    /// How many pages of rows have arrived.
    pub(crate) pages: usize,
    /// The pages of the fetch in progress. The rows on screen are
    /// replaced from here once enough of them have arrived.
    pub(crate) incoming: Vec<Schedule>,
}

impl ScheduleTable {
    pub(crate) fn selected(&self) -> Option<&Schedule> {
        self.schedules.get(self.state.selected()?)
    }
}

impl Widget for &mut ScheduleTable {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let inner = block("Schedules", self.focused, area, buf);

        // A cron expression says how it repeats but not when that
        // falls next, and the two do not fit one row side by side, so
        // the time goes underneath it.
        let policies = self
            .schedules
            .iter()
            .map(|schedule| {
                let policy = schedule
                    .schedule
                    .as_ref()
                    .and_then(|def| def.scheduling.as_ref());

                let next = (schedule.status() != ScheduleStatus::Stopped)
                    .then(|| next_fire(policy))
                    .flatten();

                (policy_summary(policy), next)
            })
            .collect::<Vec<_>>();

        // Sized to the widest of them, so that the column neither cuts
        // an expression short nor leaves a gap on a wide terminal.
        let policy_width = policies
            .iter()
            .map(|(summary, next)| {
                summary
                    .chars()
                    .count()
                    .max(next.as_ref().map_or(0, |next| next.chars().count() + 2))
            })
            .max()
            .unwrap_or(0)
            .clamp(*POLICY_WIDTH.start(), *POLICY_WIDTH.end());

        let rows = self
            .schedules
            .iter()
            .zip(&policies)
            .map(|(schedule, (summary, next))| {
                let status = schedule.status();

                let mut lines = vec![Line::from(summary.clone())];

                if let Some(next) = next {
                    lines.push(
                        Line::from(format!("↳ {next}")).style(Style::new().fg(tailwind::GRAY.c500)),
                    );
                }

                let height = u16::try_from(lines.len()).unwrap_or(1);

                Row::new([
                    Cell::new(match status {
                        ScheduleStatus::Stopped => "stopped",
                        _ => "active",
                    })
                    .style(match status {
                        ScheduleStatus::Stopped => Style::new().fg(tailwind::GRAY.c400),
                        _ => Style::new().fg(tailwind::GREEN.c400),
                    }),
                    Cell::new(Text::from(lines)),
                    Cell::new(format_age(schedule.created_at))
                        .style(Style::new().fg(tailwind::GRAY.c400)),
                    Cell::new(schedule.id.clone()),
                ])
                .height(height)
            })
            .collect::<Vec<_>>();

        let table = Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Max(u16::try_from(policy_width).unwrap_or(u16::MAX)),
                Constraint::Length(9),
                // A UUID and a column of its own to keep it off the
                // border, which the row highlight reaches.
                Constraint::Length(37),
            ],
        )
        .header(
            Row::new(["Status", "Policy", "Created", "ID"])
                .style(Style::new().bold().fg(tailwind::GRAY.c400)),
        )
        .row_highlight_style(Style::new().bg(SLATE.c800).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ")
        .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(table, inner, buf, &mut self.state);

        if self.schedules.is_empty() {
            let [_, body] =
                Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
            empty_message(self.loading, "No schedules for this job type.", body, buf);
        }
    }
}

/// A short description of when a schedule creates jobs.
pub(super) fn policy_summary(policy: Option<&SchedulingPolicy>) -> String {
    match policy.and_then(|p| p.policy.as_ref()) {
        Some(Policy::Cron(cron)) => cron.cron_expression.clone(),
        Some(Policy::Interval(interval)) => {
            format!("every {}", format_duration(interval.interval))
        }
        None => String::new(),
    }
}

/// The next time a cron schedule fires, as an absolute time
/// with a relative hint. Interval schedules are not included
/// because the server does not report their last fire time.
fn next_fire(policy: Option<&SchedulingPolicy>) -> Option<String> {
    let Some(Policy::Cron(cron)) = policy.and_then(|p| p.policy.as_ref()) else {
        return None;
    };

    let crontab = super::parse_crontab(&cron.cron_expression).ok()?;
    let now = Timestamp::now();
    let next = crontab.find_next(now).ok()?;

    Some(format!(
        "{} (in {})",
        next.strftime(TIME_FORMAT),
        compact_duration(next.timestamp().duration_since(now))
    ))
}
