//! Machine-readable output formats for the list commands.

use std::{fmt::Write, time::SystemTime};

use clap::ValueEnum;
use jiff::Timestamp;
use serde_json::{Map, Value};

/// How a list command renders its results.
#[derive(ValueEnum, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) enum OutputFormat {
    /// Human-readable table.
    #[default]
    Table,
    /// An array of one flat object per row.
    Json,
    /// Comma-separated values with a header row.
    Csv,
}

impl OutputFormat {
    /// Whether the command should render its human-readable table.
    pub(crate) fn is_table(self) -> bool {
        matches!(self, OutputFormat::Table)
    }
}

/// A single row, in the key order the command inserted its fields.
pub(crate) type Record = Map<String, Value>;

/// Print rows as JSON or CSV.
///
/// CSV uses the first row's keys as the header, so every row a command
/// produces must carry the same fields in the same order.
pub(crate) fn print_records(format: OutputFormat, records: &[Record]) -> eyre::Result<()> {
    match format {
        OutputFormat::Table => unreachable!("the caller renders its own table"),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(records)?),
        OutputFormat::Csv => print_csv(records),
    }

    Ok(())
}

fn print_csv(records: &[Record]) {
    let Some(first) = records.first() else {
        return;
    };

    let mut out = String::new();

    for (i, key) in first.keys().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_csv_field(&mut out, key);
    }
    out.push('\n');

    for record in records {
        for (i, key) in first.keys().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write_csv_field(
                &mut out,
                &csv_value(record.get(key).unwrap_or(&Value::Null)),
            );
        }
        out.push('\n');
    }

    print!("{out}");
}

/// Flatten a value into a single CSV cell.
///
/// Objects and arrays of scalars collapse to `a=1;b=2` and `a;b`; anything
/// deeper keeps its JSON so no information is silently dropped.
fn csv_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) if items.iter().all(Value::is_string) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(";"),
        Value::Object(fields) if fields.values().all(|v| !v.is_object() && !v.is_array()) => {
            let mut out = String::new();
            for (key, value) in fields {
                if !out.is_empty() {
                    out.push(';');
                }
                write!(&mut out, "{key}={}", csv_value(value)).unwrap();
            }
            out
        }
        _ => value.to_string(),
    }
}

fn write_csv_field(out: &mut String, field: &str) {
    if field.contains([',', '"', '\n', '\r']) {
        out.push('"');
        for c in field.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
    } else {
        out.push_str(field);
    }
}

/// An RFC 3339 UTC timestamp, or null.
pub(crate) fn timestamp(time: Option<SystemTime>) -> Value {
    time.and_then(|t| Timestamp::try_from(t).ok())
        .map_or(Value::Null, |ts| Value::String(ts.to_string()))
}

/// A whole number of milliseconds, or null.
pub(crate) fn millis(duration: Option<std::time::Duration>) -> Value {
    duration.map_or(Value::Null, |d| {
        Value::Number(u64::try_from(d.as_millis()).unwrap_or(u64::MAX).into())
    })
}

/// Labels as a `key` to `value` object.
pub(crate) fn labels<'a>(labels: impl IntoIterator<Item = (&'a str, &'a str)>) -> Value {
    Value::Object(
        labels
            .into_iter()
            .map(|(key, value)| (key.to_owned(), Value::String(value.to_owned())))
            .collect(),
    )
}
