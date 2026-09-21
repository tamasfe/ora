mod detail;
mod executors;
mod form;
mod job_types;
mod jobs;
mod schedules;

use std::time::SystemTime;

pub(crate) use detail::Detail;
pub(crate) use executors::ExecutorTable;
pub(crate) use form::{Form, FormKind, parse_cron};
use jiff::{SignedDuration, Timestamp, TimestampRound};
pub(crate) use job_types::JobTypeList;
pub(crate) use jobs::JobTable;
use ora::{JobOrderBy, ScheduleOrderBy, proto::admin::v1::ExecutionStatus};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Style, palette::tailwind},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};
pub(crate) use schedules::ScheduleTable;
use serde_json::Value;

use crate::tui::{App, Confirm, Tab};

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl Widget for &mut App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let main_layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ]);

        let [tabs, content, footer] = main_layout.areas(area);

        render_tabs(self, tabs, buf);

        self.job_type_list.loading = !self.pending.job_types.idle();
        self.job_table.loading = self.pending.for_tab(Tab::Jobs);
        self.schedule_table.loading = self.pending.for_tab(Tab::Schedules);
        self.executor_table.loading = self.pending.for_tab(Tab::Executors);

        match self.tab {
            Tab::Jobs => {
                let [left, right] = job_type_layout(self.job_type_list.max_width()).areas(content);
                self.job_type_list.render(left, buf);
                self.job_table.render(right, buf);
            }
            Tab::Schedules => {
                let [left, right] = job_type_layout(self.job_type_list.max_width()).areas(content);
                self.job_type_list.render(left, buf);
                self.schedule_table.render(right, buf);
            }
            Tab::Executors => {
                self.executor_table.render(content, buf);
            }
        }

        render_footer(self, footer, buf);

        let tab = self.tab;
        if let Some(detail) = self.details[tab.index()].as_mut() {
            detail.render(content, buf);
        }

        // Fullscreen: the form rebinds tab, so a tab bar would be a lie.
        if let Some(form) = self.form.as_mut() {
            form.render(area, buf);
        }

        if let Some(confirm) = self.confirm.as_ref() {
            render_confirm(confirm, content, buf);
        }
    }
}

fn job_type_layout(max_width: u16) -> Layout {
    Layout::horizontal([Constraint::Length(max_width + 5), Constraint::Fill(1)])
}

fn render_tabs(app: &App, area: Rect, buf: &mut ratatui::prelude::Buffer) {
    let mut line = Line::default();

    for (tab, name) in [
        (Tab::Jobs, "Jobs"),
        (Tab::Schedules, "Schedules"),
        (Tab::Executors, "Executors"),
    ] {
        let style = if tab == app.tab {
            Style::new().bold().fg(tailwind::BLUE.c400)
        } else {
            Style::new().fg(tailwind::GRAY.c500)
        };

        line.push_span(Span::from(format!(" {name} ")).style(style));
    }

    line.push_span(Span::from(" (tab)").style(Style::new().fg(tailwind::GRAY.c600)));

    // The tabs keep their width; the actions give way instead.
    let tabs_width = u16::try_from(line.width())
        .unwrap_or(u16::MAX)
        .min(area.width);

    let [left, right] =
        Layout::horizontal([Constraint::Length(tabs_width), Constraint::Fill(1)]).areas(area);

    let mut actions = action_spans(app);
    let width = |spans: &[Span<'static>]| spans.iter().map(Span::width).sum::<usize>();

    // Drop whole entries rather than cut one in half.
    while !actions.is_empty() && width(&actions) > right.width as usize {
        actions.remove(0);

        if actions.first().is_some_and(|span| span.content.trim().is_empty()) {
            actions.remove(0);
        }
    }

    line.render(left, buf);
    Line::from(actions).right_aligned().render(right, buf);
}

/// What the keys do, least steady first: the row is right aligned, so
/// only what is left of a changing action moves.
fn action_spans(app: &App) -> Vec<Span<'static>> {
    // An overlay owns the keyboard and carries its own hints.
    if app.modal_open() {
        return Vec::new();
    }

    let mut spans = Vec::new();
    let mut action = |text: &'static str| {
        if !spans.is_empty() {
            spans.push(Span::from("  "));
        }

        spans.push(Span::from(text));
    };

    match app.tab {
        Tab::Jobs => {
            if app.can_cancel_job() {
                action("cancel (c)");
            }
        }
        Tab::Schedules => {
            if app.can_stop_schedule() {
                action("stop (s)");
            }
        }
        Tab::Executors => {}
    }

    if app.can_duplicate() {
        action("duplicate (d)");
    }

    if app.can_create() {
        action("new (n)");
    }

    action("refresh (r)");

    // The row has no margin of its own to keep it off the edge.
    spans.push(Span::from(" "));

    spans.into_iter()
        .map(|span| span.style(Style::new().fg(tailwind::GRAY.c500)))
        .collect()
}

fn render_footer(app: &App, area: Rect, buf: &mut ratatui::prelude::Buffer) {
    // An error says more than the filters do, so give it up to two thirds.
    let status = status_line(app, (area.width / 3 * 2) as usize);
    let status_width = u16::try_from(status.width())
        .unwrap_or(u16::MAX)
        .min(area.width);

    let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(status_width)])
        .horizontal_margin(1)
        .areas(area);

    Line::from(filter_spans(app)).render(left, buf);
    status.right_aligned().render(right, buf);
}

const SEPARATOR: &str = " · ";

/// What the rows are filtered by, next to the key that changes each
/// one, and the label filter as it is typed.
fn filter_spans(app: &App) -> Vec<Span<'static>> {
    if app.modal_open() {
        return Vec::new();
    }

    // Typing takes the row, which fits several pairs and their syntax.
    if app.labels_focused {
        let orange = Style::new().fg(tailwind::ORANGE.c600);

        return vec![
            Span::from("labels: ").style(orange.bold()),
            Span::from(format!("{}_", app.label_filter(app.tab))).style(orange),
            Span::from("   key=value or key, comma separated   enter apply   esc clear")
                .style(Style::new().fg(tailwind::GRAY.c600)),
        ];
    }

    let (status, order) = match app.tab {
        Tab::Jobs => (
            app.job_status.label(),
            match app.job_order {
                JobOrderBy::CreatedAtDesc => "created ↓",
                JobOrderBy::CreatedAtAsc => "created ↑",
                JobOrderBy::TargetExecutionTimeAsc => "target ↑",
                JobOrderBy::TargetExecutionTimeDesc => "target ↓",
            },
        ),
        Tab::Schedules => (
            app.schedule_status.label(),
            match app.schedule_order {
                ScheduleOrderBy::CreatedAtDesc => "created ↓",
                ScheduleOrderBy::CreatedAtAsc => "created ↑",
            },
        ),
        // Nothing filters the executors.
        Tab::Executors => return Vec::new(),
    };

    let dim = Style::new().fg(tailwind::GRAY.c500);
    let mut spans = vec![
        Span::from(format!("{status} (a)")).style(dim),
        Span::from(SEPARATOR).style(dim),
        Span::from(format!("{order} (o)")).style(dim),
        Span::from(SEPARATOR).style(dim),
    ];

    if app.label_filter(app.tab).is_empty() {
        spans.push(Span::from("labels (l)").style(dim));
    } else {
        spans.push(Span::from(app.label_filter(app.tab).to_string()).style(Style::new().bold()));
        spans.push(Span::from(" (l)").style(dim));
    }

    spans
}

fn status_line(app: &App, max_error: usize) -> Line<'static> {
    let mut spans = Vec::new();

    if let Some(waited) = app.pending.waited() {
        spans.push(
            Span::from(format!(
                "{} {}s ",
                SPINNER[app.spinner % SPINNER.len()],
                waited.as_secs()
            ))
            .style(Style::new().fg(tailwind::BLUE.c400)),
        );
    }

    if let Some(error) = app.status.error.as_ref() {
        spans.push(
            Span::from(truncate(error, max_error)).style(Style::new().fg(tailwind::RED.c400)),
        );
    } else {
        // Since the server last answered anything, not since these rows.
        let age = match app.status.updated_at {
            Some(updated_at) => format!("updated {}s ago", updated_at.elapsed().as_secs()),
            None => "connecting…".to_string(),
        };

        spans.push(Span::from(age).style(Style::new().fg(tailwind::GRAY.c500)));
    }

    Line::from(spans)
}

/// Shorten text to a number of characters, never splitting
/// a multi-byte character.
fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_string(),
    }
}

fn render_confirm(confirm: &Confirm, area: Rect, buf: &mut ratatui::prelude::Buffer) {
    let [area] = Layout::horizontal([Constraint::Max(70)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(5)])
        .flex(Flex::Center)
        .areas(area);

    Clear.render(area, buf);

    Paragraph::new(vec![
        Line::from(confirm.prompt.clone()),
        Line::default(),
        Line::from("(y) confirm   (n) cancel").style(Style::new().fg(tailwind::GRAY.c500)),
    ])
    .wrap(Wrap { trim: true })
    .block(
        Block::new()
            .title(" Confirm ")
            .title_style(Style::new().bold().fg(tailwind::ORANGE.c400))
            .borders(Borders::all())
            .border_style(Style::new().fg(tailwind::ORANGE.c400))
            .padding(ratatui::widgets::Padding::horizontal(1)),
    )
    .render(area, buf);
}

/// Convert a protobuf timestamp into a [`jiff::Timestamp`].
pub(super) fn timestamp_of<T>(ts: T) -> Option<Timestamp>
where
    T: TryInto<SystemTime>,
{
    Timestamp::try_from(ts.try_into().ok()?).ok()
}

/// Format a protobuf timestamp as an absolute time,
/// an empty string if it is missing or invalid.
pub(super) fn format_time<T>(ts: Option<T>) -> String
where
    T: TryInto<SystemTime>,
{
    let Some(ts) = ts.and_then(timestamp_of) else {
        return String::new();
    };

    ts.round(TimestampRound::new().smallest(jiff::Unit::Second))
        .unwrap_or(ts)
        .strftime(TIME_FORMAT)
        .to_string()
}

/// Format a protobuf timestamp as an absolute time followed by
/// its age, e.g. `2026-09-21 09:12:00 (3m ago)`.
pub(super) fn format_time_with_age<T>(ts: Option<T>) -> String
where
    T: TryInto<SystemTime> + Copy,
{
    let time = format_time(ts);

    if time.is_empty() {
        return time;
    }

    format!("{time} ({})", format_age(ts))
}

/// Format a protobuf timestamp relative to now, e.g. `3m ago`.
pub(super) fn format_age<T>(ts: Option<T>) -> String
where
    T: TryInto<SystemTime>,
{
    let Some(ts) = ts.and_then(timestamp_of) else {
        return String::new();
    };

    let elapsed = Timestamp::now().duration_since(ts);

    if elapsed.is_negative() {
        return format!("in {}", compact_duration(-elapsed));
    }

    format!("{} ago", compact_duration(elapsed))
}

/// Format a duration as a short, single-unit approximation.
pub(super) fn compact_duration(duration: SignedDuration) -> String {
    let secs = duration.as_secs();

    // Rounded rather than truncated, so that a time two hours out reads
    // as `2h` and not as `1h`.
    match secs {
        ..60 => format!("{secs}s"),
        60..3600 => format!("{}m", (secs + 30) / 60),
        3600..86400 => format!("{}h", (secs + 1800) / 3600),
        _ => format!("{}d", (secs + 43200) / 86400),
    }
}

/// Format a protobuf duration in a human-readable form.
pub(super) fn format_duration<T>(duration: Option<T>) -> String
where
    T: TryInto<std::time::Duration>,
{
    duration
        .and_then(|d| d.try_into().ok())
        .map(|d| humantime::format_duration(d).to_string())
        .unwrap_or_default()
}

/// Pretty-print a JSON string, returning it unchanged if it is not valid JSON.
pub(super) fn pretty_json(json: &str) -> String {
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| json.to_string())
}

pub(super) fn execution_status_label(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Unspecified | ExecutionStatus::Pending => "pending",
        ExecutionStatus::InProgress => "in-progress",
        ExecutionStatus::Succeeded => "succeeded",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
    }
}

pub(super) fn execution_status_style(status: ExecutionStatus) -> Style {
    match status {
        ExecutionStatus::Succeeded => Style::new().fg(tailwind::GREEN.c400),
        ExecutionStatus::Failed | ExecutionStatus::Cancelled => Style::new().fg(tailwind::RED.c400),
        ExecutionStatus::InProgress => Style::new().fg(tailwind::YELLOW.c400),
        _ => Style::new().fg(tailwind::GRAY.c400),
    }
}

/// A labelled line used throughout the detail views.
pub(super) fn field(name: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::from(format!("{name}: ")).style(Style::new().bold()),
        Span::from(value.into()),
    ])
}

/// Explain why a table has no rows, so that a pending request
/// is never mistaken for a result set that came back empty.
pub(super) fn empty_message(
    loading: bool,
    empty: &str,
    area: Rect,
    buf: &mut ratatui::prelude::Buffer,
) {
    let text = if loading { "Loading…" } else { empty };

    Paragraph::new(Line::from(text.to_string()))
        .style(Style::new().fg(tailwind::GRAY.c500))
        .block(Block::new().padding(ratatui::widgets::Padding::left(2)))
        .render(area, buf);
}

/// Render a bordered block and return the area inside it.
pub(super) fn block(
    title: &str,
    focused: bool,
    area: Rect,
    buf: &mut ratatui::prelude::Buffer,
) -> Rect {
    let block = Block::new()
        .title(format!(" {title} "))
        .title_style(Style::new().bold())
        .borders(Borders::all())
        .border_style(if focused {
            Style::new().fg(tailwind::BLUE.c400)
        } else {
            Style::default()
        });

    let inner = block.inner(area);
    block.render(area, buf);
    inner
}

/// Parse a cron expression, defaulting to UTC when it names no timezone.
pub(super) fn parse_crontab(expression: &str) -> Result<cronexpr::Crontab, cronexpr::Error> {
    let mut options = cronexpr::ParseOptions::default();
    options.fallback_timezone_option = cronexpr::FallbackTimezoneOption::UTC;

    cronexpr::parse_crontab_with(expression, options)
}
