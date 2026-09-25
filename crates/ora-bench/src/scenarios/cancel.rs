//! Cancellation latency.

use std::time::{Instant, SystemTime};

use ora::proto::admin::v1::JobFilters;
use serde::Serialize;

use crate::{
    executor::{Event, Mode},
    scenarios::{Ctx, Run},
    stats::{Recorder, ScenarioReport},
};

/// Options for the `cancel` scenario.
#[derive(Debug, Clone, clap::Args, Serialize)]
pub struct CancelArgs {
    /// The number of jobs to measure.
    #[arg(long, default_value_t = 200)]
    pub jobs: usize,
}

impl Default for CancelArgs {
    fn default() -> Self {
        Self { jobs: 200 }
    }
}

/// Cancel in-progress jobs and measure how long it takes
/// for the executor to be notified.
pub async fn run(ctx: &mut Ctx, run: &Run, args: &CancelArgs) -> eyre::Result<ScenarioReport> {
    let mut cancel_rpc = Recorder::new("CancelJobs rpc", ctx.warmup);
    let mut cancel_to_notified = Recorder::new("cancel → executor notified", ctx.warmup);

    for _ in 0..ctx.warmup + args.jobs {
        let ids = ctx
            .add_jobs(vec![run.job(SystemTime::now(), Mode::Hold)])
            .await?;
        let id = ids[0].clone();

        let execution = ctx
            .wait_for(|ev| match ev {
                Event::Ready {
                    job_id,
                    execution_id,
                    ..
                } if job_id == id => Some(execution_id),
                _ => None,
            })
            .await?;

        let t0 = Instant::now();
        let cancelled = ctx
            .cancel(JobFilters {
                job_ids: vec![id],
                ..Default::default()
            })
            .await?;
        cancel_rpc.record(t0.elapsed());

        if cancelled != 1 {
            eyre::bail!("expected 1 job to be cancelled, got {cancelled}");
        }

        let notified_at = ctx
            .wait_for(|ev| match ev {
                Event::Cancelled { execution_id, at } if execution_id == execution => Some(at),
                _ => None,
            })
            .await?;
        cancel_to_notified.record(notified_at - t0);
    }

    let mut report = ScenarioReport::new(
        "cancel",
        "Closed loop: add a job that never completes, cancel it once in progress, measure time until the executor is notified.",
    );
    report.latencies = vec![cancel_rpc.summary(), cancel_to_notified.summary()];
    Ok(report)
}
