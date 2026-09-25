//! Lateness of jobs scheduled in the future.

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

use serde::Serialize;

use crate::{
    executor::{Event, Mode},
    scenarios::{Ctx, Run},
    stats::{Metric, Recorder, ScenarioReport},
};

/// Options for the `delayed` scenario.
#[derive(Debug, Clone, clap::Args, Serialize)]
pub struct DelayedArgs {
    /// The number of jobs to measure.
    #[arg(long, default_value_t = 500)]
    pub jobs: usize,
    /// The delay of the first job's target execution time, in milliseconds.
    #[arg(long, default_value_t = 2000)]
    pub delay_ms: u64,
    /// Target execution times are spread evenly over this window, in milliseconds.
    #[arg(long, default_value_t = 1000)]
    pub spread_ms: u64,
    /// The number of jobs added in a single request.
    #[arg(long, default_value_t = 100)]
    pub batch_size: usize,
}

impl Default for DelayedArgs {
    fn default() -> Self {
        Self {
            jobs: 500,
            delay_ms: 2000,
            spread_ms: 1000,
            batch_size: 100,
        }
    }
}

/// Add jobs due in the future and measure how late
/// they arrive at the executor.
pub async fn run(ctx: &mut Ctx, run: &Run, args: &DelayedArgs) -> eyre::Result<ScenarioReport> {
    let mut lateness = Recorder::new("target time → ready", 0);

    let base = SystemTime::now() + Duration::from_millis(args.delay_ms);
    let step = Duration::from_millis(args.spread_ms) / u32::try_from(args.jobs.max(1))?;
    let targets: Vec<SystemTime> = (0..args.jobs)
        .map(|i| base + step * u32::try_from(i).unwrap_or(u32::MAX))
        .collect();

    let submit_start = Instant::now();
    let mut pending: HashMap<String, SystemTime> = HashMap::with_capacity(args.jobs);
    for batch in targets.chunks(args.batch_size.max(1)) {
        let jobs = batch.iter().map(|t| run.job(*t, Mode::Succeed)).collect();
        let ids = ctx.add_jobs(jobs).await?;
        pending.extend(ids.into_iter().zip(batch.iter().copied()));
    }
    let submit_time = submit_start.elapsed();

    if SystemTime::now() > base {
        tracing::warn!(
            ?submit_time,
            "submitting took longer than the delay, increase --delay-ms for accurate results"
        );
    }

    let mut early = 0;
    let timeout = Duration::from_millis(args.delay_ms + args.spread_ms) + Duration::from_secs(30);
    let deadline = Instant::now() + timeout;

    while !pending.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if let Event::Ready {
            job_id, wall_at, ..
        } = ctx.next_event(remaining).await?
            && let Some(target) = pending.remove(&job_id)
        {
            match wall_at.duration_since(target) {
                Ok(late) => lateness.record(late),
                Err(_) => early += 1,
            }
        }
    }

    let mut report = ScenarioReport::new(
        "delayed",
        "Add jobs due in the future, measure how late they reach the executor. \
         Includes clock skew between the benchmark and the server.",
    );
    report.latencies = vec![lateness.summary()];
    report.metrics = vec![
        Metric::duration("submit time", submit_time),
        Metric::count("jobs received early", early),
    ];
    Ok(report)
}
