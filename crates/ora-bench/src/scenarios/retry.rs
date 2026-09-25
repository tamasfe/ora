//! Retry latency.

use std::time::{Instant, SystemTime};

use serde::Serialize;

use crate::{
    executor::{Event, Mode},
    scenarios::{Ctx, Run},
    stats::{Recorder, ScenarioReport},
};

/// Options for the `retry` scenario.
#[derive(Debug, Clone, clap::Args, Serialize)]
pub struct RetryArgs {
    /// The number of jobs to measure.
    #[arg(long, default_value_t = 200)]
    pub jobs: usize,
}

impl Default for RetryArgs {
    fn default() -> Self {
        Self { jobs: 200 }
    }
}

/// Submit jobs that fail on the first attempt and measure
/// how long it takes for the retry to reach the executor.
pub async fn run(ctx: &mut Ctx, run: &Run, args: &RetryArgs) -> eyre::Result<ScenarioReport> {
    let mut first_ready = Recorder::new("submit → first attempt ready", ctx.warmup);
    let mut retry_ready = Recorder::new("failure sent → retry ready", ctx.warmup);

    for _ in 0..ctx.warmup + args.jobs {
        let t0 = Instant::now();
        let ids = ctx
            .add_jobs(vec![run.job(SystemTime::now(), Mode::FailFirst)])
            .await?;
        let id = &ids[0];

        let ready_at = ctx
            .wait_for(|ev| match ev {
                Event::Ready {
                    job_id,
                    attempt: 1,
                    at,
                    ..
                } if job_id == *id => Some(at),
                _ => None,
            })
            .await?;
        first_ready.record(ready_at - t0);

        let failed_at = ctx
            .wait_for(|ev| match ev {
                Event::FailedSent { job_id, at } if job_id == *id => Some(at),
                _ => None,
            })
            .await?;

        let retried_at = ctx
            .wait_for(|ev| match ev {
                Event::Ready {
                    job_id,
                    attempt: 2,
                    at,
                    ..
                } if job_id == *id => Some(at),
                _ => None,
            })
            .await?;
        retry_ready.record(retried_at - failed_at);
    }

    let mut report = ScenarioReport::new(
        "retry",
        "Closed loop: add a job that fails its first attempt (1 retry, no backoff), measure time until the retry arrives.",
    );
    report.latencies = vec![first_ready.summary(), retry_ready.summary()];
    Ok(report)
}
