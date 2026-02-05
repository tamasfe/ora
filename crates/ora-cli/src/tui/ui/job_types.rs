use std::cmp;

use ora::admin::job_types::JobTypeInfo;
use ratatui::{
    style::{
        Modifier, Style,
        palette::tailwind::{self, SLATE},
    },
    symbols,
    widgets::{
        Block, Borders, HighlightSpacing, List, ListItem, ListState, StatefulWidget, Widget,
    },
};

#[derive(Debug, Default)]
pub(crate) struct JobTypeList {
    pub(crate) focused: bool,
    pub(crate) state: ListState,
    pub(crate) job_types: Vec<JobTypeInfo>,
}

impl JobTypeList {
    pub(crate) fn max_width(&self) -> u16 {
        cmp::min(
            50,
            self.job_types
                .iter()
                .map(|jt| u16::try_from(jt.id.as_str().len()).unwrap_or(10))
                .max()
                .unwrap_or(10),
        )
    }
}

impl Widget for &mut JobTypeList {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        const SELECTED_STYLE: Style = Style::new().bg(SLATE.c800).add_modifier(Modifier::BOLD);

        let block = Block::new()
            .title(" Types ")
            .title_style(Style::new().bold())
            .borders(Borders::all())
            .border_set(symbols::border::PLAIN)
            .border_style(if self.focused {
                Style::new().fg(tailwind::BLUE.c400)
            } else {
                Style::default()
            });

        let items = self
            .job_types
            .iter()
            .map(|job_type| ListItem::new(job_type.id.as_str()))
            .collect::<Vec<_>>();

        let list = List::new(items)
            .block(block)
            .highlight_style(SELECTED_STYLE)
            .highlight_symbol("> ")
            .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(list, area, buf, &mut self.state);
    }
}
