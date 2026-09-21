use jiff::Timestamp;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style, palette::tailwind},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};
use serde_json::{Map, Value};

/// How much room the field names get before their values.
const LABEL_WIDTH: usize = 28;

/// The widest a key column is allowed to grow.
const KEY_WIDTH: usize = 32;

/// The caret and the text around it, windowed to `width` characters so
/// the caret is always on screen. Returns the spans, how many
/// characters they show, and whether the window starts after the
/// text's first character (there is more of it scrolled off to the
/// left).
fn caret_window(
    text: &str,
    cursor: usize,
    width: usize,
    style: Style,
) -> (Vec<Span<'static>>, usize, bool) {
    let chars: Vec<char> = text.chars().collect();
    let start = (cursor + 1).saturating_sub(width);
    let end = (start + width).min(chars.len());

    let before: String = chars[start..cursor.min(end)].iter().collect();
    let at = chars.get(cursor).copied().unwrap_or(' ');
    let after: String = if cursor < end {
        chars[cursor + 1..end].iter().collect()
    } else {
        String::new()
    };

    let shown = before.chars().count() + 1 + after.chars().count();

    let spans = vec![
        Span::from(before).style(style),
        Span::from(at.to_string()).style(style.add_modifier(Modifier::REVERSED)),
        Span::from(after).style(style),
    ];

    (spans, shown, start > 0)
}

/// One editable cell, padded to `width`, showing the caret when it is
/// the active one.
fn cell_spans(text: &str, width: usize, active: bool, cursor: usize) -> Vec<Span<'static>> {
    let style = if active {
        Style::new().fg(tailwind::ORANGE.c400)
    } else {
        Style::new()
    };

    if !active {
        let shown = super::truncate(text, width);
        let pad = width.saturating_sub(shown.chars().count());
        return vec![Span::from(format!("{shown}{}", " ".repeat(pad))).style(style)];
    }

    let (mut spans, shown, _) = caret_window(text, cursor, width.max(4), style);
    spans.push(Span::from(" ".repeat(width.saturating_sub(shown))));
    spans
}

/// The marker, label, required star and value of one form row.
fn row_spans(
    label: &str,
    required: bool,
    value: &str,
    selected: bool,
    cursor: Option<usize>,
    budget: usize,
) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::from(if selected { "> " } else { "  " }),
        Span::from(format!("{label:<LABEL_WIDTH$}")),
        Span::from(if required { "* " } else { "  " })
            .style(Style::new().fg(tailwind::ORANGE.c400)),
    ];

    let value_style = if selected {
        Style::new().fg(tailwind::ORANGE.c400)
    } else {
        Style::new()
    };

    let Some(cursor) = cursor else {
        spans.push(
            Span::from(if value.is_empty() {
                "-".to_string()
            } else {
                super::truncate(value, budget)
            })
            .style(value_style),
        );

        return spans;
    };

    // Scroll the row so the caret is always on screen, wherever in the
    // text it happens to be.
    let (window, _, truncated) = caret_window(value, cursor, budget.max(8), value_style);

    if truncated {
        spans.push(Span::from("…").style(Style::new().fg(tailwind::GRAY.c600)));
    }

    spans.extend(window);
    spans
}

/// Whether the form creates a job or a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormKind {
    Job,
    Schedule,
}

/// Which part of the form a field belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Derived from the job type's input schema.
    Input,
    /// Job or schedule options that are not part of the payload.
    Options,
}

/// The editor a field uses, chosen from its schema.
#[derive(Debug, Clone)]
enum FieldKind {
    Text,
    Integer,
    Number,
    Bool,
    Choice(Vec<String>),
    /// A list of scalars, edited one value per row. Carries the
    /// schema's declared type for its items.
    List(ListItemKind),
    /// An instant, entered as a timestamp, a date, `now` or `+1h`.
    Time,
    /// A cron expression.
    Cron,
    /// Key and value pairs, edited as two cells per row.
    Pairs,
}

/// The scalar type a list field's items parse to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListItemKind {
    Text,
    Integer,
    Number,
    Bool,
}

/// The byte offset of a character position.
fn byte_at(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(index, _)| index)
}

/// The useful part of a cronexpr parse error.
///
/// It reports the problem as a caret diagram over several lines, which
/// cannot be shown on one row, so take the explanation it ends with.
fn cron_error(error: &cronexpr::Error) -> String {
    let text = error.to_string();

    text.lines()
        .last()
        .and_then(|line| line.split_once('^'))
        .map(|(_, message)| message.trim().to_string())
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "invalid cron expression".to_string())
}

/// The next time a cron expression fires, used both to validate it and
/// to show what it actually means.
pub(crate) fn parse_cron(text: &str) -> Result<Timestamp, String> {
    let crontab = super::parse_crontab(text.trim()).map_err(|error| cron_error(&error))?;

    crontab
        .find_next(Timestamp::now())
        .map(|zoned| zoned.timestamp())
        .map_err(|_| "never fires".to_string())
}

/// Parse a time as typed into a form field.
///
/// Accepts an RFC3339 timestamp, a plain `YYYY-MM-DD` date, `now`, and
/// a `+` prefixed duration such as `+2h` meaning that long from now.
pub(crate) fn parse_time(text: &str) -> Result<Timestamp, String> {
    let text = text.trim();

    if text.eq_ignore_ascii_case("now") {
        return Ok(Timestamp::now());
    }

    if let Some(duration) = text.strip_prefix('+') {
        let duration = humantime::parse_duration(duration.trim())
            .map_err(|error| format!("invalid duration: {error}"))?;

        let duration = jiff::SignedDuration::try_from(duration)
            .map_err(|_| "duration is too large".to_string())?;

        return Timestamp::now()
            .checked_add(duration)
            .map_err(|_| "time is out of range".to_string());
    }

    if let Ok(timestamp) = text.parse::<Timestamp>() {
        return Ok(timestamp);
    }

    jiff::fmt::strtime::parse("%Y-%m-%d", text)
        .and_then(|parsed| parsed.to_date())
        .map_err(|_| "use a date, a timestamp, `now` or `+1h`".to_string())?
        .to_datetime(jiff::civil::Time::midnight())
        .in_tz("UTC")
        .map(|zoned| zoned.timestamp())
        .map_err(|_| "date is out of range".to_string())
}

#[derive(Debug, Clone)]
struct Field {
    /// Where the value belongs in the payload, e.g. `["input", "project_id"]`.
    /// Option fields use a single synthetic key.
    path: Vec<String>,
    label: String,
    description: Option<String>,
    required: bool,
    section: Section,
    kind: FieldKind,
    /// Holds the value for text and integer fields.
    text: String,
    /// One entry per row for list fields, never an empty one: the row
    /// being typed into is the blank row shown after the last.
    items: Vec<String>,
    /// One key and value per row for pair fields, on the same terms.
    pairs: Vec<(String, String)>,
    toggled: bool,
    choice: usize,
}

impl Field {
    fn option(key: &str, label: &str, kind: FieldKind, description: &str) -> Self {
        Self {
            path: vec![key.to_string()],
            label: label.to_string(),
            description: Some(description.to_string()),
            required: false,
            section: Section::Options,
            kind,
            text: String::new(),
            items: Vec::new(),
            pairs: Vec::new(),
            toggled: false,
            choice: 0,
        }
    }

    /// How the current value is shown in the field list.
    fn display(&self) -> String {
        match &self.kind {
            FieldKind::Bool => if self.toggled { "yes" } else { "no" }.to_string(),
            FieldKind::Choice(choices) => choices
                .get(self.choice)
                .cloned()
                .unwrap_or_else(|| "-".to_string()),
            FieldKind::List(_) => self.items.join(", "),
            FieldKind::Pairs => self
                .pairs
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(", "),
            _ => self.text.clone(),
        }
    }

    /// The value to put in the payload, `None` when the field is
    /// empty and may be left out.
    fn value(&self) -> Option<Value> {
        match &self.kind {
            FieldKind::Bool => Some(Value::Bool(self.toggled)),
            FieldKind::Choice(choices) => {
                choices.get(self.choice).map(|c| Value::String(c.clone()))
            }
            FieldKind::List(item_kind) => {
                let items: Vec<Value> = self
                    .items
                    .iter()
                    .map(|item| item.trim())
                    .filter(|item| !item.is_empty())
                    .filter_map(|item| match item_kind {
                        ListItemKind::Text => Some(Value::String(item.to_string())),
                        ListItemKind::Integer => item.parse::<i64>().ok().map(Value::from),
                        ListItemKind::Number => item.parse::<f64>().ok().map(Value::from),
                        ListItemKind::Bool => item.parse::<bool>().ok().map(Value::Bool),
                    })
                    .collect();

                // An empty required list is meaningful, several schemas
                // treat it as "everything".
                if items.is_empty() && !self.required {
                    None
                } else {
                    Some(Value::Array(items))
                }
            }
            FieldKind::Integer => self.text.trim().parse::<i64>().ok().map(Value::from),
            FieldKind::Number => self.text.trim().parse::<f64>().ok().map(Value::from),
            FieldKind::Time => {
                if self.text.trim().is_empty() {
                    None
                } else {
                    parse_time(&self.text)
                        .ok()
                        .map(|timestamp| Value::String(timestamp.to_string()))
                }
            }
            // Pairs only ever hold options, never a payload value.
            FieldKind::Pairs => None,
            FieldKind::Text | FieldKind::Cron => {
                if self.text.trim().is_empty() {
                    None
                } else {
                    Some(Value::String(self.text.clone()))
                }
            }
        }
    }

    fn editable_text(&self) -> bool {
        matches!(
            self.kind,
            FieldKind::Text
                | FieldKind::Integer
                | FieldKind::Number
                | FieldKind::List(_)
                | FieldKind::Time
                | FieldKind::Cron
                | FieldKind::Pairs
        )
    }
}

/// A schema driven form for creating a job or a schedule.
#[derive(Debug)]
pub(crate) struct Form {
    pub(crate) kind: FormKind,
    pub(crate) job_type_id: String,
    schema: Option<Value>,
    fields: Vec<Field>,
    selected: usize,
    /// The row within the selected field, only lists have more than one.
    item: usize,
    /// Where the caret sits in the text being edited, in characters.
    cursor: usize,
    /// Which cell of a pair row is being edited: 0 is key, 1 is value.
    cell: usize,
    pub(crate) error: Option<String>,
    pub(crate) submitting: bool,
}

impl Form {
    pub(crate) fn new(kind: FormKind, job_type_id: String, input_schema: Option<&str>) -> Self {
        let schema = input_schema.and_then(|raw| serde_json::from_str::<Value>(raw).ok());

        let mut fields = schema.as_ref().map(fields_from_schema).unwrap_or_default();
        fields.extend(option_fields(kind));

        Self {
            kind,
            job_type_id,
            schema,
            fields,
            selected: 0,
            item: 0,
            cursor: 0,
            cell: 0,
            error: None,
            submitting: false,
        }
    }

    /// The text the caret is in, if the selected row holds any.
    fn active_text(&self) -> Option<&String> {
        let field = self.fields.get(self.selected)?;

        match field.kind {
            FieldKind::List(_) => field.items.get(self.item),
            FieldKind::Pairs => field
                .pairs
                .get(self.item)
                .map(|pair| if self.cell == 0 { &pair.0 } else { &pair.1 }),
            FieldKind::Text
            | FieldKind::Integer
            | FieldKind::Number
            | FieldKind::Time
            | FieldKind::Cron => Some(&field.text),
            _ => None,
        }
    }

    /// Put the caret at the end of whatever row is now selected.
    fn reset_cursor(&mut self) {
        self.cursor = self.active_text().map_or(0, |text| text.chars().count());
    }

    /// How many rows a field occupies. A list gets one row per value
    /// plus a blank one to type the next into, while it has the cursor.
    fn rows_in(&self, index: usize) -> usize {
        match self.fields.get(index) {
            Some(field) if matches!(field.kind, FieldKind::List(_)) => field.items.len() + 1,
            Some(field) if matches!(field.kind, FieldKind::Pairs) => field.pairs.len() + 1,
            _ => 1,
        }
    }

    /// Drop the blank rows of a list once the cursor leaves it.
    fn leave_field(&mut self) {
        if let Some(field) = self.fields.get_mut(self.selected) {
            field.items.retain(|item| !item.trim().is_empty());
            field
                .pairs
                .retain(|(key, value)| !key.trim().is_empty() || !value.trim().is_empty());
        }
    }

    /// Tab steps through the cells of a pair row before moving on.
    pub(crate) fn select_next_cell(&mut self) {
        let pairs = self
            .fields
            .get(self.selected)
            .is_some_and(|field| matches!(field.kind, FieldKind::Pairs));

        if pairs && self.cell == 0 {
            self.cell = 1;
            self.reset_cursor();
            return;
        }

        self.select_next();
    }

    /// Shift tab reverses that walk, ending on the value of the row
    /// above rather than skipping over it.
    pub(crate) fn select_previous_cell(&mut self) {
        let pairs = self
            .fields
            .get(self.selected)
            .is_some_and(|field| matches!(field.kind, FieldKind::Pairs));

        if pairs && self.cell == 1 {
            self.cell = 0;
            self.reset_cursor();
            return;
        }

        self.select_previous();

        if self
            .fields
            .get(self.selected)
            .is_some_and(|field| matches!(field.kind, FieldKind::Pairs))
        {
            self.cell = 1;
            self.reset_cursor();
        }
    }

    pub(crate) fn select_next(&mut self) {
        if self.fields.is_empty() {
            return;
        }

        if self.item + 1 < self.rows_in(self.selected) {
            self.item += 1;
            self.cell = 0;
            self.reset_cursor();
            return;
        }

        self.leave_field();
        self.selected = (self.selected + 1) % self.fields.len();
        self.item = 0;
        self.cell = 0;
        self.reset_cursor();
    }

    pub(crate) fn select_previous(&mut self) {
        if self.fields.is_empty() {
            return;
        }

        if self.item > 0 {
            self.item -= 1;
            self.cell = 0;
            self.reset_cursor();
            return;
        }

        self.leave_field();
        self.selected = self
            .selected
            .checked_sub(1)
            .unwrap_or(self.fields.len() - 1);
        self.item = self.rows_in(self.selected).saturating_sub(1);
        self.cell = 0;
        self.reset_cursor();
    }

    /// Left and right move the caret in a text row, and change the
    /// value of a boolean or a choice, which have no caret.
    pub(crate) fn cycle(&mut self, forward: bool) {
        let len = self.active_text().map_or(0, |text| text.chars().count());
        let pairs = matches!(
            self.fields.get(self.selected).map(|field| &field.kind),
            Some(FieldKind::Pairs)
        );

        if pairs && forward && self.cell == 0 && self.cursor >= len {
            self.cell = 1;
            self.cursor = 0;
            return;
        }

        if pairs && !forward && self.cell == 1 && self.cursor == 0 {
            self.cell = 0;
            self.reset_cursor();
            return;
        }

        let Some(field) = self.fields.get_mut(self.selected) else {
            return;
        };

        match &field.kind {
            FieldKind::Bool => field.toggled = !field.toggled,
            FieldKind::Choice(choices) if !choices.is_empty() => {
                field.choice = if forward {
                    (field.choice + 1) % choices.len()
                } else {
                    field.choice.checked_sub(1).unwrap_or(choices.len() - 1)
                };
            }
            _ => {
                self.cursor = if forward {
                    (self.cursor + 1).min(len)
                } else {
                    self.cursor.saturating_sub(1)
                };
            }
        }
    }

    /// Move the caret a word left or right, the way a shell does.
    pub(crate) fn move_word(&mut self, forward: bool) {
        let cursor = self.active_text().map(|text| {
            if forward {
                word_end(text, self.cursor)
            } else {
                word_start(text, self.cursor)
            }
        });

        if let Some(cursor) = cursor {
            self.cursor = cursor;
        }
    }

    /// Delete back to the start of the word before the caret.
    pub(crate) fn pop_word(&mut self) {
        let target = self
            .active_text()
            .map_or(self.cursor, |text| word_start(text, self.cursor));

        // No word boundary to delete back to, so take one character.
        if target == self.cursor {
            self.pop_char();
            return;
        }

        for _ in target..self.cursor {
            self.pop_char();
        }
    }

    pub(crate) fn push_char(&mut self, c: char) {
        let Some(field) = self.fields.get_mut(self.selected) else {
            return;
        };

        match field.kind {
            FieldKind::List(_) => {
                // Brackets and quotes belong to JSON rather than to a
                // value, so pasting an array still yields clean rows.
                if matches!(c, '[' | ']' | '"') {
                    return;
                }

                // A comma finishes this value and starts the next one.
                if c == ',' {
                    if self.item < field.items.len() {
                        self.item += 1;
                        self.cursor = field.items.get(self.item).map_or(0, |i| i.chars().count());
                    }

                    return;
                }

                if self.item >= field.items.len() {
                    field.items.push(String::new());
                }

                if let Some(item) = field.items.get_mut(self.item) {
                    let at = byte_at(item, self.cursor);
                    item.insert(at, c);
                    self.cursor += 1;
                }
            }
            FieldKind::Pairs => {
                // `=` is the separator, so it moves to the value
                // instead of being typed into the key.
                if c == '=' && self.cell == 0 {
                    self.cell = 1;
                    self.cursor = field
                        .pairs
                        .get(self.item)
                        .map_or(0, |p| p.1.chars().count());
                    return;
                }

                // A comma finishes this pair and starts the next one.
                if c == ',' {
                    if self.item < field.pairs.len() {
                        self.item += 1;
                        self.cell = 0;
                        self.cursor = field
                            .pairs
                            .get(self.item)
                            .map_or(0, |p| p.0.chars().count());
                    }

                    return;
                }

                if self.item >= field.pairs.len() {
                    field.pairs.push((String::new(), String::new()));
                }

                if let Some(pair) = field.pairs.get_mut(self.item) {
                    let text = if self.cell == 0 {
                        &mut pair.0
                    } else {
                        &mut pair.1
                    };

                    let at = byte_at(text, self.cursor);
                    text.insert(at, c);
                    self.cursor += 1;
                }
            }
            FieldKind::Text
            | FieldKind::Integer
            | FieldKind::Number
            | FieldKind::Time
            | FieldKind::Cron => {
                let at = byte_at(&field.text, self.cursor);
                field.text.insert(at, c);
                self.cursor += 1;
            }
            // Space flips a checkbox, and has no text here to land in.
            FieldKind::Bool if c == ' ' => field.toggled = !field.toggled,
            _ => {}
        }
    }

    /// Remove the character the caret sits on.
    pub(crate) fn delete_char(&mut self) {
        let Some(field) = self.fields.get_mut(self.selected) else {
            return;
        };

        match field.kind {
            FieldKind::List(_) => {
                let Some(item) = field.items.get_mut(self.item) else {
                    return;
                };

                let from = byte_at(item, self.cursor);
                let to = byte_at(item, self.cursor + 1);

                if from < to {
                    item.replace_range(from..to, "");
                }

                if item.is_empty() {
                    field.items.remove(self.item);
                    self.cursor = 0;
                }
            }
            FieldKind::Pairs => {
                let Some(pair) = field.pairs.get_mut(self.item) else {
                    return;
                };

                let text = if self.cell == 0 {
                    &mut pair.0
                } else {
                    &mut pair.1
                };

                let from = byte_at(text, self.cursor);
                let to = byte_at(text, self.cursor + 1);

                if from < to {
                    text.replace_range(from..to, "");
                }

                if pair.0.is_empty() && pair.1.is_empty() {
                    field.pairs.remove(self.item);
                    self.cell = 0;
                    self.cursor = 0;
                }
            }
            FieldKind::Text
            | FieldKind::Integer
            | FieldKind::Number
            | FieldKind::Time
            | FieldKind::Cron => {
                let from = byte_at(&field.text, self.cursor);
                let to = byte_at(&field.text, self.cursor + 1);

                if from < to {
                    field.text.replace_range(from..to, "");
                }
            }
            _ => {}
        }
    }

    pub(crate) fn pop_char(&mut self) {
        let Some(field) = self.fields.get_mut(self.selected) else {
            return;
        };

        match field.kind {
            FieldKind::List(_) => {
                let Some(item) = field.items.get_mut(self.item) else {
                    // Carry on at the end of the row above.
                    if self.item > 0 {
                        self.item -= 1;
                        self.cursor = field
                            .items
                            .get(self.item)
                            .map_or(0, |item| item.chars().count());
                    }

                    return;
                };

                if self.cursor == 0 {
                    return;
                }

                let from = byte_at(item, self.cursor - 1);
                let to = byte_at(item, self.cursor);
                item.replace_range(from..to, "");
                self.cursor -= 1;

                if item.is_empty() {
                    field.items.remove(self.item);
                }
            }
            FieldKind::Pairs => {
                let Some(pair) = field.pairs.get_mut(self.item) else {
                    // Carry on at the end of the row above, its value.
                    if self.item > 0 {
                        self.item -= 1;
                        self.cell = 1;
                        self.cursor = field
                            .pairs
                            .get(self.item)
                            .map_or(0, |pair| pair.1.chars().count());
                    }

                    return;
                };

                if self.cursor == 0 {
                    if self.cell == 1 {
                        self.cell = 0;
                        self.cursor = pair.0.chars().count();
                    }

                    return;
                }

                let text = if self.cell == 0 {
                    &mut pair.0
                } else {
                    &mut pair.1
                };

                let from = byte_at(text, self.cursor - 1);
                let to = byte_at(text, self.cursor);
                text.replace_range(from..to, "");
                self.cursor -= 1;

                if pair.0.is_empty() && pair.1.is_empty() {
                    field.pairs.remove(self.item);
                    self.cell = 0;
                    self.cursor = 0;
                }
            }
            FieldKind::Text
            | FieldKind::Integer
            | FieldKind::Number
            | FieldKind::Time
            | FieldKind::Cron
                if self.cursor > 0 =>
            {
                let from = byte_at(&field.text, self.cursor - 1);
                let to = byte_at(&field.text, self.cursor);
                field.text.replace_range(from..to, "");
                self.cursor -= 1;
            }
            _ => {}
        }
    }

    /// The payload built from the input fields.
    pub(crate) fn payload(&self) -> Value {
        let mut root = Value::Object(Map::new());

        for field in self.fields.iter().filter(|f| f.section == Section::Input) {
            if let Some(value) = field.value() {
                insert(&mut root, &field.path, value);
            }
        }

        root
    }

    fn option_field_mut(&mut self, key: &str) -> Option<&mut Field> {
        self.fields
            .iter_mut()
            .find(|f| f.section == Section::Options && f.path.first().is_some_and(|p| p == key))
    }

    /// Set the text of an option field.
    pub(crate) fn set_option(&mut self, key: &str, value: String) {
        if let Some(field) = self.option_field_mut(key) {
            field.text = value;
        }
    }

    /// Set whether an option field is on.
    pub(crate) fn set_option_bool(&mut self, key: &str, value: bool) {
        if let Some(field) = self.option_field_mut(key) {
            field.toggled = value;
        }
    }

    /// Set the rows of an option field that holds key and value pairs.
    pub(crate) fn set_pairs_option(&mut self, key: &str, rows: Vec<(String, String)>) {
        if let Some(field) = self.option_field_mut(key) {
            field.pairs = rows;
        }
    }

    /// Fill the input fields from a payload that already exists.
    ///
    /// Each field takes the value at its own path, so a payload
    /// written against an older schema fills in what still fits and
    /// leaves the rest empty.
    pub(crate) fn prefill_input(&mut self, payload: &str) {
        let Ok(payload) = serde_json::from_str::<Value>(payload) else {
            return;
        };

        for field in &mut self.fields {
            if field.section != Section::Input {
                continue;
            }

            let value = field.path.iter().try_fold(&payload, |node, key| node.get(key));

            apply_default(field, value);
        }
    }

    fn option_field(&self, key: &str) -> Option<&Field> {
        self.fields
            .iter()
            .find(|f| f.section == Section::Options && f.path.first().is_some_and(|p| p == key))
    }

    /// The value of an option field, empty string when unset.
    pub(crate) fn option(&self, key: &str) -> String {
        self.option_field(key)
            .map(Field::display)
            .unwrap_or_default()
    }

    /// An option field parsed as a time, `None` when it is empty.
    pub(crate) fn time_option(&self, key: &str) -> Result<Option<Timestamp>, String> {
        let text = self.option(key);
        let text = text.trim();

        if text.is_empty() {
            return Ok(None);
        }

        parse_time(text)
            .map(Some)
            .map_err(|error| format!("{key}: {error}"))
    }

    /// The rows of an option field that holds key and value pairs.
    pub(crate) fn pairs_option(&self, key: &str) -> Vec<(String, String)> {
        self.option_field(key)
            .map(|field| field.pairs.clone())
            .unwrap_or_default()
    }

    pub(crate) fn option_bool(&self, key: &str) -> bool {
        self.option_field(key).is_some_and(|field| field.toggled)
    }

    /// Check required fields and the schema, returning the first problem.
    pub(crate) fn validate(&self) -> Option<String> {
        for field in self.fields.iter().filter(|f| f.section == Section::Input) {
            if field.required && field.value().is_none() {
                return Some(format!("{} is required", field.label));
            }

            if matches!(field.kind, FieldKind::Time)
                && !field.text.trim().is_empty()
                && let Err(error) = parse_time(&field.text)
            {
                return Some(format!("{}: {error}", field.label));
            }

            if matches!(field.kind, FieldKind::Integer)
                && !field.text.trim().is_empty()
                && field.text.trim().parse::<i64>().is_err()
            {
                return Some(format!("{} must be a whole number", field.label));
            }

            if matches!(field.kind, FieldKind::Number)
                && !field.text.trim().is_empty()
                && field.text.trim().parse::<f64>().is_err()
            {
                return Some(format!("{} must be a number", field.label));
            }
        }

        let schema = self.schema.as_ref()?;

        if jsonschema::is_valid(schema, &self.payload()) {
            None
        } else {
            Some("payload does not match the input schema".to_string())
        }
    }
}

/// The job or schedule options shown below the payload fields.
fn option_fields(kind: FormKind) -> Vec<Field> {
    let mut fields = match kind {
        FormKind::Job => vec![Field::option(
            "target",
            "target time",
            FieldKind::Time,
            "When the job should run. Empty means now.",
        )],
        FormKind::Schedule => vec![
            Field::option(
                "cron",
                "cron",
                FieldKind::Cron,
                "Cron expression, e.g. `0 * * * *`. Leave empty to use an interval instead.",
            ),
            Field::option(
                "interval",
                "interval",
                FieldKind::Text,
                "Repeat interval, e.g. `30m`. Used when no cron expression is given.",
            ),
            Field::option(
                "immediate",
                "run immediately",
                FieldKind::Bool,
                "Create a job as soon as the schedule is added.",
            ),
        ],
    };

    fields.push(Field::option(
        "labels",
        "labels",
        FieldKind::Pairs,
        "A key and a value per row.",
    ));
    fields.push(Field::option(
        "timeout",
        "timeout",
        FieldKind::Text,
        "How long a run may take, e.g. `2h`. Empty means no timeout.",
    ));
    fields.push(Field::option(
        "retries",
        "retries",
        FieldKind::Integer,
        "How many times to retry a failed run.",
    ));

    fields
}

/// Build the editable fields for a job type's input schema.
/// How many nested objects `collect_fields` will descend into.
///
/// A self-referential schema (a `$defs` entry whose object properties
/// resolve back to itself) would otherwise recurse without bound and
/// crash the TUI; nothing legitimate nests this deep.
const MAX_SCHEMA_DEPTH: usize = 8;

fn fields_from_schema(schema: &Value) -> Vec<Field> {
    let mut fields = Vec::new();
    collect_fields(schema, schema, &[], 0, &mut fields);
    fields
}

fn collect_fields(root: &Value, node: &Value, path: &[String], depth: usize, out: &mut Vec<Field>) {
    if depth > MAX_SCHEMA_DEPTH {
        return;
    }

    let required: Vec<&str> = node
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let Some(properties) = node.get("properties").and_then(Value::as_object) else {
        return;
    };

    for (name, property) in properties {
        let property = resolve(root, property);
        let (kind_name, _) = type_of(root, &property);

        let mut path = path.to_vec();
        path.push(name.clone());

        // A nested object becomes its own group of fields rather than
        // one field holding raw JSON.
        if kind_name.as_deref() == Some("object") && property.get("properties").is_some() {
            collect_fields(root, &property, &path, depth + 1, out);
            continue;
        }

        let kind = field_kind(root, &property, kind_name.as_deref());
        let label = path.join(".");

        let mut field = Field {
            path,
            label,
            description: property
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            required: required.contains(&name.as_str()),
            section: Section::Input,
            kind,
            text: String::new(),
            items: Vec::new(),
            pairs: Vec::new(),
            toggled: false,
            choice: 0,
        };

        apply_default(&mut field, property.get("default"));
        out.push(field);
    }
}

/// The values a node is allowed to take, written either as an `enum`
/// or, the way a documented Rust enum is derived, as a branch per
/// variant holding a single `const`.
fn choices_of(root: &Value, node: &Value) -> Option<Vec<String>> {
    if let Some(choices) = node.get("enum").and_then(Value::as_array) {
        return Some(choices.iter().map(literal).collect());
    }

    let branches: Vec<Value> = node
        .get("oneOf")
        .or_else(|| node.get("anyOf"))?
        .as_array()?
        .iter()
        .map(|branch| resolve(root, branch))
        .filter(|branch| branch.get("type").and_then(Value::as_str) != Some("null"))
        .collect();

    // A nullable enum leaves one branch, which holds the variants.
    if let [only] = branches.as_slice()
        && only.get("const").is_none()
    {
        return choices_of(root, only);
    }

    branches
        .iter()
        .map(|branch| branch.get("const").map(literal))
        .collect::<Option<Vec<_>>>()
        .filter(|choices| !choices.is_empty())
}

/// The start of the word before the caret, skipping any separators
/// in between, or the start of the text.
fn word_start(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut at = cursor.min(chars.len());

    while at > 0 && !chars[at - 1].is_alphanumeric() {
        at -= 1;
    }

    while at > 0 && chars[at - 1].is_alphanumeric() {
        at -= 1;
    }

    at
}

/// The end of the word after the caret, or the end of the text.
fn word_end(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut at = cursor.min(chars.len());

    while at < chars.len() && !chars[at].is_alphanumeric() {
        at += 1;
    }

    while at < chars.len() && chars[at].is_alphanumeric() {
        at += 1;
    }

    at
}

/// A JSON scalar as it is written in a form.
fn literal(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// The declared format of a node, looking through a nullable `anyOf`.
fn format_of(root: &Value, node: &Value) -> Option<String> {
    if let Some(format) = node.get("format").and_then(Value::as_str) {
        return Some(format.to_string());
    }

    node.get("anyOf")?
        .as_array()?
        .iter()
        .map(|branch| resolve(root, branch))
        .find(|branch| branch.get("type").and_then(Value::as_str) != Some("null"))
        .and_then(|branch| {
            branch
                .get("format")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn field_kind(root: &Value, property: &Value, kind_name: Option<&str>) -> FieldKind {
    if let Some(choices) = choices_of(root, property) {
        return FieldKind::Choice(choices);
    }

    if matches!(
        format_of(root, property).as_deref(),
        Some("date-time" | "date")
    ) {
        return FieldKind::Time;
    }

    match kind_name {
        Some("boolean") => FieldKind::Bool,
        Some("integer") => FieldKind::Integer,
        Some("number") => FieldKind::Number,
        Some("array") => FieldKind::List(list_item_kind(root, property)),
        _ => FieldKind::Text,
    }
}

/// The scalar type of an array field's items, `Text` when the schema
/// says nothing more specific.
fn list_item_kind(root: &Value, property: &Value) -> ListItemKind {
    let Some(items) = property.get("items") else {
        return ListItemKind::Text;
    };

    let items = resolve(root, items);
    let (kind_name, _) = type_of(root, &items);

    match kind_name.as_deref() {
        Some("boolean") => ListItemKind::Bool,
        Some("integer") => ListItemKind::Integer,
        Some("number") => ListItemKind::Number,
        _ => ListItemKind::Text,
    }
}

fn apply_default(field: &mut Field, default: Option<&Value>) {
    let Some(default) = default else {
        return;
    };

    match (&field.kind, default) {
        (FieldKind::Bool, Value::Bool(value)) => field.toggled = *value,
        (FieldKind::Choice(choices), Value::String(value)) => {
            if let Some(index) = choices.iter().position(|choice| choice == value) {
                field.choice = index;
            }
        }
        (FieldKind::List(_), Value::Array(items)) => {
            field.items = items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        (_, Value::String(value)) => field.text.clone_from(value),
        (_, Value::Number(value)) => field.text = value.to_string(),
        _ => {}
    }
}

/// Follow a local `$ref` into `$defs`, keeping any sibling keys such as
/// `default` and `description` that sit next to the reference.
fn resolve(root: &Value, node: &Value) -> Value {
    let Some(reference) = node.get("$ref").and_then(Value::as_str) else {
        return node.clone();
    };

    let Some(target) = reference
        .strip_prefix("#/$defs/")
        .and_then(|name| root.get("$defs").and_then(|defs| defs.get(name)))
    else {
        return node.clone();
    };

    let mut merged = target.clone();

    if let (Some(target), Some(source)) = (merged.as_object_mut(), node.as_object()) {
        for (key, value) in source {
            if key != "$ref" {
                target.insert(key.clone(), value.clone());
            }
        }
    }

    merged
}

/// The type of a schema node, ignoring a null alternative, plus whether
/// null is allowed.
fn type_of(root: &Value, node: &Value) -> (Option<String>, bool) {
    if let Some(branches) = node.get("anyOf").and_then(Value::as_array) {
        let branches: Vec<Value> = branches.iter().map(|b| resolve(root, b)).collect();
        let nullable = branches
            .iter()
            .any(|b| b.get("type").and_then(Value::as_str) == Some("null"));
        let first = branches
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) != Some("null"));

        return (
            first
                .and_then(|b| b.get("type"))
                .and_then(Value::as_str)
                .map(str::to_string),
            nullable,
        );
    }

    match node.get("type") {
        Some(Value::String(name)) => (Some(name.clone()), false),
        Some(Value::Array(names)) => {
            let nullable = names.iter().any(|n| n.as_str() == Some("null"));
            let first = names
                .iter()
                .find_map(|n| n.as_str().filter(|name| *name != "null"));

            (first.map(str::to_string), nullable)
        }
        _ => (None, false),
    }
}

/// Put a value at a dotted path, creating the objects along the way.
fn insert(root: &mut Value, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };

    let mut node = root;

    for key in parents {
        node = node
            .as_object_mut()
            .expect("payload nodes are objects")
            .entry(key.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }

    if let Some(object) = node.as_object_mut() {
        object.insert(last.clone(), value);
    }
}

/// Show what a cron or time field's text parses to, or the parse
/// error, appended as a hint after its value.
fn push_parsed_preview(
    spans: &mut Vec<Span<'static>>,
    result: Result<Timestamp, String>,
    prefix: &str,
) {
    let (text, style) = match result {
        Ok(timestamp) => (
            format!(
                "  → {prefix}{} ({})",
                super::format_time(Some(timestamp)),
                super::format_age(Some(timestamp))
            ),
            Style::new().fg(tailwind::GRAY.c500),
        ),
        Err(error) => (format!("  {error}"), Style::new().fg(tailwind::RED.c400)),
    };

    spans.push(Span::from(text).style(style));
}

impl Widget for &mut Form {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        Clear.render(area, buf);

        let title = match self.kind {
            FormKind::Job => format!(" New job · {} ", self.job_type_id),
            FormKind::Schedule => format!(" New schedule · {} ", self.job_type_id),
        };

        let block = Block::new()
            .title(title)
            .title_style(Style::new().bold().fg(tailwind::GREEN.c400))
            .borders(Borders::all())
            .border_style(Style::new().fg(tailwind::GREEN.c400))
            .padding(ratatui::widgets::Padding::horizontal(1));

        let inner = block.inner(area);
        block.render(area, buf);

        let [list, footer] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).areas(inner);

        let mut lines = Vec::new();
        let mut section = None;
        let mut cursor_line = 0;

        for (index, field) in self.fields.iter().enumerate() {
            if section != Some(field.section) {
                if section.is_some() {
                    lines.push(Line::default());
                }

                lines.push(
                    Line::from(match field.section {
                        Section::Input => "Input",
                        Section::Options => match self.kind {
                            FormKind::Job => "Job",
                            FormKind::Schedule => "Schedule",
                        },
                    })
                    .style(Style::new().bold().fg(tailwind::GRAY.c400)),
                );

                section = Some(field.section);
            }

            let field_selected = index == self.selected;
            let preview = if matches!(field.kind, FieldKind::Time | FieldKind::Cron) {
                46
            } else {
                18
            };
            let budget = usize::from(list.width).saturating_sub(LABEL_WIDTH + preview);

            if matches!(field.kind, FieldKind::Pairs) {
                // No rows and no cursor, so it reads as any empty field.
                if !field_selected && field.pairs.is_empty() {
                    lines.push(Line::from(row_spans(
                        &field.label,
                        field.required,
                        "",
                        false,
                        None,
                        budget,
                    )));

                    continue;
                }

                let rows = if field_selected && self.item >= field.pairs.len() {
                    field.pairs.len() + 1
                } else {
                    field.pairs.len()
                };

                // Aligned, but only as far right as the longest key needs.
                let key_width = field
                    .pairs
                    .iter()
                    .map(|(key, _)| key.chars().count())
                    .max()
                    .unwrap_or(0)
                    .min(KEY_WIDTH)
                    + 1;

                for row in 0..rows {
                    let selected = field_selected && row == self.item;
                    let (key, value) = field.pairs.get(row).cloned().unwrap_or_default();

                    let mut spans = vec![
                        Span::from(if selected { "> " } else { "  " }),
                        Span::from(format!(
                            "{:<LABEL_WIDTH$}  ",
                            if row == 0 { field.label.as_str() } else { "" }
                        )),
                    ];

                    let key_active = selected && self.cell == 0;
                    let key_gap = usize::from(key_active && self.cursor >= key.chars().count());

                    spans.extend(cell_spans(
                        &key,
                        key_width + key_gap,
                        key_active,
                        self.cursor,
                    ));

                    if selected || !value.trim().is_empty() {
                        spans.push(Span::from("= ").style(Style::new().fg(tailwind::GRAY.c600)));
                        spans.extend(cell_spans(
                            &value,
                            budget.saturating_sub(key_width + 2),
                            selected && self.cell == 1,
                            self.cursor,
                        ));
                    }

                    if selected {
                        cursor_line = lines.len();
                    }

                    lines.push(Line::from(spans).style(if selected {
                        Style::new().add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    }));
                }

                continue;
            }

            if matches!(field.kind, FieldKind::List(_)) {
                let rows = if field_selected && self.item >= field.items.len() {
                    field.items.len() + 1
                } else {
                    field.items.len().max(1)
                };

                for row in 0..rows {
                    let selected = field_selected && row == self.item;
                    let value = field.items.get(row).cloned().unwrap_or_default();

                    let mut spans = row_spans(
                        if row == 0 { &field.label } else { "" },
                        field.required && row == 0,
                        &value,
                        selected,
                        selected.then_some(self.cursor),
                        budget,
                    );

                    if row == 0 {
                        let count = field.items.iter().filter(|i| !i.trim().is_empty()).count();

                        spans.push(
                            Span::from(match count {
                                0 => "  empty".to_string(),
                                1 => "  1 value".to_string(),
                                n => format!("  {n} values"),
                            })
                            .style(Style::new().fg(tailwind::GRAY.c600)),
                        );
                    }

                    if selected {
                        cursor_line = lines.len();
                    }

                    lines.push(Line::from(spans).style(if selected {
                        Style::new().add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    }));
                }

                continue;
            }

            let selected = field_selected;
            let editing = selected && field.editable_text();

            let mut spans = row_spans(
                &field.label,
                field.required,
                &field.display(),
                selected,
                editing.then_some(self.cursor),
                budget,
            );

            // Show what it parsed to, so a typo shows before submitting.
            if matches!(field.kind, FieldKind::Cron) && !field.text.trim().is_empty() {
                push_parsed_preview(&mut spans, parse_cron(&field.text), "next ");
            }

            if matches!(field.kind, FieldKind::Time) && !field.text.trim().is_empty() {
                push_parsed_preview(&mut spans, parse_time(&field.text), "");
            }

            // Neither shows as anything but its value, so the row
            // has to say that it is cycled rather than typed into.
            if selected && matches!(field.kind, FieldKind::Choice(_) | FieldKind::Bool) {
                spans.push(Span::from("  ←/→").style(Style::new().fg(tailwind::GRAY.c600)));
            }

            if selected {
                cursor_line = lines.len();
            }

            lines.push(Line::from(spans).style(if selected {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            }));
        }

        // A list can grow past the bottom of the pane, so keep the row
        // being edited on screen.
        let offset = cursor_line.saturating_sub(usize::from(list.height).saturating_sub(1));

        Paragraph::new(lines)
            .scroll((u16::try_from(offset).unwrap_or(0), 0))
            .render(list, buf);

        let mut help = Vec::new();

        if let Some(error) = self.error.as_ref() {
            help.push(Line::from(error.clone()).style(Style::new().fg(tailwind::RED.c400)));
        } else if let Some(field) = self.fields.get(self.selected) {
            let mut text = field
                .description
                .clone()
                .unwrap_or_default()
                .replace('\n', " ");

            let hint = match field.kind {
                FieldKind::List(_) => {
                    "Press `,` or ↓ for another value. Emptying a row removes it."
                }
                FieldKind::Pairs => {
                    "Tab moves to the value, ↓ adds another pair. Emptying a row removes it."
                }
                FieldKind::Time => {
                    "Write 2026-09-21, 2026-09-21T14:30:00Z, `now`, or `+2h` for a time from now."
                }
                _ => "",
            };

            if !hint.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }

                text.push_str(hint);
            }

            help.push(Line::from(text).style(Style::new().fg(tailwind::GRAY.c400)));
        } else {
            help.push(Line::default());
        }

        help.push(Line::default());
        help.push(
            Line::from(if self.submitting {
                "submitting…".to_string()
            } else {
                "↑/↓ field   ←/→ change   enter create   esc cancel".to_string()
            })
            .style(Style::new().fg(tailwind::GRAY.c500)),
        );

        Paragraph::new(help)
            .wrap(Wrap { trim: true })
            .render(footer, buf);
    }
}
