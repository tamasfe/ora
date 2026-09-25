//! Metric collection and reporting.

use std::time::Duration;

use comfy_table::{Cell, CellAlignment, Table, presets::UTF8_FULL_CONDENSED};
use hdrhistogram::Histogram;
use serde::Serialize;

/// Records latency samples into a histogram,
/// discarding the first `warmup` samples.
pub struct Recorder {
    name: String,
    warmup: usize,
    hist: Histogram<u64>,
}

impl Recorder {
    /// Create a recorder for the given metric.
    pub fn new(name: impl Into<String>, warmup: usize) -> Self {
        Self {
            name: name.into(),
            warmup,
            // 1µs to 1h with 3 significant digits.
            hist: Histogram::new_with_bounds(1, 3_600_000_000, 3).unwrap(),
        }
    }

    /// Record a sample.
    pub fn record(&mut self, value: Duration) {
        if self.warmup > 0 {
            self.warmup -= 1;
            return;
        }

        let micros = u64::try_from(value.as_micros()).unwrap_or(u64::MAX);
        self.hist.saturating_record(micros.max(1));
    }

    /// Summarize the recorded samples.
    pub fn summary(&self) -> LatencySummary {
        let h = &self.hist;
        let empty = h.is_empty();
        let q = |q: f64| if empty { 0 } else { h.value_at_quantile(q) };

        LatencySummary {
            name: self.name.clone(),
            count: h.len(),
            min_us: if empty { 0 } else { h.min() },
            mean_us: if empty { 0.0 } else { h.mean() },
            p50_us: q(0.5),
            p90_us: q(0.9),
            p99_us: q(0.99),
            p999_us: q(0.999),
            max_us: if empty { 0 } else { h.max() },
            throughput_per_sec: None,
        }
    }
}

/// Summary of a latency distribution, all values in microseconds.
#[derive(Debug, Clone, Serialize)]
pub struct LatencySummary {
    pub name: String,
    pub count: u64,
    pub min_us: u64,
    pub mean_us: f64,
    pub p50_us: u64,
    pub p90_us: u64,
    pub p99_us: u64,
    pub p999_us: u64,
    pub max_us: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput_per_sec: Option<f64>,
}

impl LatencySummary {
    /// Attach a throughput value (operations per second).
    pub fn with_throughput(mut self, count: usize, elapsed: Duration) -> Self {
        self.throughput_per_sec = Some(rate(count, elapsed));
        self
    }
}

/// A single scalar metric.
#[derive(Debug, Clone, Serialize)]
pub struct Metric {
    pub name: &'static str,
    pub value: f64,
    pub unit: &'static str,
}

impl Metric {
    /// A duration metric, stored in milliseconds.
    pub fn duration(name: &'static str, value: Duration) -> Self {
        Self {
            name,
            value: value.as_secs_f64() * 1000.0,
            unit: "ms",
        }
    }

    /// A rate metric, in operations per second.
    pub fn rate(name: &'static str, count: usize, elapsed: Duration) -> Self {
        Self {
            name,
            value: rate(count, elapsed),
            unit: "/s",
        }
    }

    /// A plain count.
    pub fn count(name: &'static str, count: usize) -> Self {
        Self {
            name,
            #[allow(clippy::cast_precision_loss)]
            value: count as f64,
            unit: "",
        }
    }
}

/// The results of a single scenario.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioReport {
    pub name: &'static str,
    pub description: &'static str,
    pub latencies: Vec<LatencySummary>,
    pub metrics: Vec<Metric>,
}

impl ScenarioReport {
    /// Create an empty report.
    pub fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            latencies: Vec::new(),
            metrics: Vec::new(),
        }
    }

    /// Print the report as tables to stdout.
    pub fn print(&self) {
        println!("\n== {} ==\n{}", self.name, self.description);

        if !self.latencies.is_empty() {
            let mut table = Table::new();
            table.load_preset(UTF8_FULL_CONDENSED).set_header([
                "metric", "count", "min", "mean", "p50", "p90", "p99", "p99.9", "max", "rate",
            ]);

            for l in &self.latencies {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let mean = l.mean_us.round() as u64;
                table.add_row(
                    [
                        l.name.clone(),
                        l.count.to_string(),
                        fmt_us(l.min_us),
                        fmt_us(mean),
                        fmt_us(l.p50_us),
                        fmt_us(l.p90_us),
                        fmt_us(l.p99_us),
                        fmt_us(l.p999_us),
                        fmt_us(l.max_us),
                        l.throughput_per_sec
                            .map(|r| format!("{r:.1}/s"))
                            .unwrap_or_default(),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let cell = Cell::new(v);
                        if i == 0 {
                            cell
                        } else {
                            cell.set_alignment(CellAlignment::Right)
                        }
                    }),
                );
            }

            println!("{table}");
        }

        if !self.metrics.is_empty() {
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL_CONDENSED)
                .set_header(["metric", "value"]);

            for m in &self.metrics {
                let value = match m.unit {
                    "ms" => fmt_ms(m.value),
                    "" => format!("{}", m.value),
                    unit => format!("{:.1}{unit}", m.value),
                };
                table.add_row([
                    Cell::new(m.name),
                    Cell::new(value).set_alignment(CellAlignment::Right),
                ]);
            }

            println!("{table}");
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn rate(count: usize, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs == 0.0 {
        0.0
    } else {
        count as f64 / secs
    }
}

#[allow(clippy::cast_precision_loss)]
fn fmt_us(us: u64) -> String {
    if us < 1_000 {
        format!("{us}µs")
    } else {
        fmt_ms(us as f64 / 1000.0)
    }
}

fn fmt_ms(ms: f64) -> String {
    if ms < 1_000.0 {
        format!("{ms:.2}ms")
    } else {
        format!("{:.2}s", ms / 1000.0)
    }
}
