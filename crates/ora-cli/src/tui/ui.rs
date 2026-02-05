mod job_types;
mod jobs;

pub(crate) use job_types::JobTypeList;
pub(crate) use jobs::JobTable;
use ora::JobOrderBy;
use ratatui::{
    layout::{Constraint, Layout},
    style::palette::tailwind,
    text::{Line, Span, Text},
    widgets::Widget,
};

use crate::tui::App;

impl Widget for &mut App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let main_layout = Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]);

        let [content, footer] = main_layout.areas(area);

        let layout = Layout::horizontal([
            Constraint::Length(self.job_type_list.max_width() + 5),
            Constraint::Fill(1),
        ]);
        let [left, right] = layout.areas(content);

        self.job_type_list.render(left, buf);
        self.job_table.render(right, buf);

        let footer_layout = Layout::horizontal([
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Fill(1),
        ])
        .spacing(2)
        .horizontal_margin(1);
        let [active_only, order, labels] = footer_layout.areas(footer);

        Text::from(if self.active_only {
            "active (a)"
        } else {
            "all (a)"
        })
        .render(active_only, buf);

        Text::from(match self.job_order {
            JobOrderBy::CreatedAtDesc => "created ↓ (o)",
            JobOrderBy::CreatedAtAsc => "created ↑ (o)",
            JobOrderBy::TargetExecutionTimeAsc => "target ↑ (o)",
            JobOrderBy::TargetExecutionTimeDesc => "target ↓ (o)",
        })
        .render(order, buf);

        let mut line = Line::default();

        line.push_span(Span::from("labels (l)").style(if self.labels_focused {
            ratatui::style::Style::new()
                .bold()
                .fg(tailwind::ORANGE.c600)
        } else {
            ratatui::style::Style::new()
        }));

        if self.labels_focused {
            line.push_span(
                Span::from(format!(": {}_", self.label_filter))
                    .style(ratatui::style::Style::new().fg(tailwind::ORANGE.c600)),
            );
        } else if !self.label_filter.is_empty() {
            line.push_span(
                Span::from(format!(": {}", self.label_filter))
                    .style(ratatui::style::Style::new().bold()),
            );
        }

        line.render(labels, buf);
    }
}
