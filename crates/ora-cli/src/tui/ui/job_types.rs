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

use crate::tui::ui::empty_message;

/// Keeps the column wide enough for the placeholder message while the
/// job types are still loading or failed to load.
const MIN_WIDTH: u16 = 16;

#[derive(Debug, Default)]
pub(crate) struct JobTypeList {
    pub(crate) focused: bool,
    pub(crate) loading: bool,
    pub(crate) state: ListState,
    pub(crate) job_types: Vec<JobTypeInfo>,
}

impl JobTypeList {
    pub(crate) fn max_width(&self) -> u16 {
        self.job_types
            .iter()
            .map(|jt| u16::try_from(jt.id.as_str().len()).unwrap_or(MIN_WIDTH))
            .max()
            .unwrap_or(0)
            .clamp(MIN_WIDTH, 50)
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

        let inner = block.inner(area);

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

        if self.job_types.is_empty() {
            empty_message(self.loading, "No job types.", inner, buf);
        }
    }
}
