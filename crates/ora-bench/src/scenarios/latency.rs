//! Idle scheduling latency.

use std::time::{Instant, SystemTime};

use serde::Serialize;

use crate::{
    executor::{Event, Mode},
    scenarios::{Ctx, Run},
    stats::{Metric, Recorder, ScenarioReport},
};

/// Options for the `latency` scenario.
#[derive(Debug, Clone, clap::Args, Serialize)]
pub struct LatencyArgs {
    /// The number of jobs to measure.
    #[arg(long, default_value_t = 500)]
    pub jobs: usize,
}

impl Default for LatencyArgs {
    fn default() -> Self {
        Self { jobs: 500 }
    }
}

/// Submit one job at a time for immediate execution and wait until
/// the executor receives it.
pub async fn run(ctx: &mut Ctx, run: &Run, args: &LatencyArgs) -> eyre::Result<ScenarioReport> {
    let mut add_rpc = Recorder::new("AddJobs rpc", ctx.warmup);
    let mut submit_to_ready = Recorder::new("submit → ready", ctx.warmup);

    let start = Instant::now();

    for _ in 0..ctx.warmup + args.jobs {
        let t0 = Instant::now();
        let ids = ctx
            .add_jobs(vec![run.job(SystemTime::now(), Mode::Succeed)])
            .await?;
        add_rpc.record(t0.elapsed());

        let ready_at = ctx
            .wait_for(|ev| match ev {
                Event::Ready { job_id, at, .. } if job_id == ids[0] => Some(at),
                _ => None,
            })
            .await?;
        submit_to_ready.record(ready_at - t0);
    }

    let mut report = ScenarioReport::new(
        "latency",
        "Closed loop: add a single job due now, wait until the executor receives it, repeat.",
    );
    report.latencies = vec![add_rpc.summary(), submit_to_ready.summary()];
    report.metrics = vec![Metric::duration("total time", start.elapsed())];
    Ok(report)
}
