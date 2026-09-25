//! Throughput under load.

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

use futures::StreamExt;
use ora::proto::admin::v1::{CountJobsRequest, ExecutionStatus};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::{
    executor::{Event, Mode},
    scenarios::{Ctx, Run, add_jobs},
    stats::{Metric, Recorder, ScenarioReport},
};

/// Options for the `throughput` scenario.
#[derive(Debug, Clone, clap::Args, Serialize)]
pub struct ThroughputArgs {
    /// The number of jobs to add.
    #[arg(long, default_value_t = 10_000)]
    pub jobs: usize,
    /// The number of jobs added in a single request.
    #[arg(long, default_value_t = 100)]
    pub batch_size: usize,
    /// The number of concurrent `AddJobs` requests.
    #[arg(long, default_value_t = 4)]
    pub concurrency: usize,
    /// Give up after this many seconds.
    #[arg(long, default_value_t = 300)]
    pub timeout_secs: u64,
}

impl Default for ThroughputArgs {
    fn default() -> Self {
        Self {
            jobs: 10_000,
            batch_size: 100,
            concurrency: 4,
            timeout_secs: 300,
        }
    }
}

/// Add a lot of jobs concurrently and measure how fast they are
/// accepted, dispatched and completed.
pub async fn run(ctx: &mut Ctx, run: &Run, args: &ThroughputArgs) -> eyre::Result<ScenarioReport> {
    let mut add_rpc = Recorder::new("AddJobs rpc (batch)", 0);
    let mut submit_to_ready = Recorder::new("submit → ready", 0);

    let batch_size = args.batch_size.max(1);
    let batches: Vec<usize> = (0..args.jobs)
        .step_by(batch_size)
        .map(|start| batch_size.min(args.jobs - start))
        .collect();

    let (batch_send, mut batch_recv) = mpsc::unbounded_channel();

    let start = Instant::now();

    let producer = tokio::spawn({
        let admin = ctx.admin.clone();
        let concurrency = args.concurrency.max(1);
        let jobs: Vec<_> = batches
            .into_iter()
            .map(|n| {
                (0..n)
                    .map(|_| run.job(SystemTime::now(), Mode::Succeed))
                    .collect()
            })
            .collect();

        async move {
            let mut results = futures::stream::iter(jobs)
                .map(|jobs| {
                    let mut admin = admin.clone();
                    async move {
                        let t0 = Instant::now();
                        add_jobs(&mut admin, jobs)
                            .await
                            .map(|ids| (t0, t0.elapsed(), ids))
                    }
                })
                .buffer_unordered(concurrency);

            while let Some(res) = results.next().await {
                if batch_send.send(res).is_err() {
                    break;
                }
            }
        }
    });

    // Submission time of each job by ID.
    let mut submitted: HashMap<String, Instant> = HashMap::with_capacity(args.jobs);
    // Ready events that arrived before the `AddJobs` response.
    let mut early_ready: HashMap<String, Instant> = HashMap::new();
    let mut submitted_count = 0;
    let mut submit_time = Duration::ZERO;
    let mut ready_count = 0;
    let mut dispatch_time = Duration::ZERO;
    let mut producer_done = false;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(args.timeout_secs);

    while ready_count < args.jobs {
        tokio::select! {
            batch = batch_recv.recv(), if !producer_done => {
                let Some(batch) = batch else {
                    producer_done = true;
                    continue;
                };
                let (t0, elapsed, ids) = batch?;
                add_rpc.record(elapsed);
                submitted_count += ids.len();
                if submitted_count == args.jobs {
                    submit_time = start.elapsed();
                }
                for id in ids {
                    if let Some(at) = early_ready.remove(&id) {
                        submit_to_ready.record(at.saturating_duration_since(t0));
                        ready_count += 1;
                        dispatch_time = at - start;
                    } else {
                        submitted.insert(id, t0);
                    }
                }
            }
            ev = ctx.events.recv() => {
                let Some(ev) = ev else {
                    eyre::bail!("the benchmark executor disconnected");
                };
                if let Event::Ready { job_id, at, .. } = ev {
                    if let Some(t0) = submitted.remove(&job_id) {
                        submit_to_ready.record(at - t0);
                        ready_count += 1;
                        dispatch_time = at - start;
                    } else {
                        early_ready.insert(job_id, at);
                    }
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                eyre::bail!("timed out, {ready_count}/{} jobs received", args.jobs);
            }
        }
    }

    producer.await?;

    // Completion is only observable by polling.
    let mut filters = run.filters();
    filters.execution_statuses = vec![ExecutionStatus::Succeeded.into()];
    let completion_time = loop {
        let count = ctx
            .admin
            .count_jobs(CountJobsRequest {
                filters: Some(filters.clone()),
            })
            .await?
            .into_inner()
            .count;

        if count >= args.jobs as u64 {
            break start.elapsed();
        }

        if tokio::time::Instant::now() > deadline {
            eyre::bail!("timed out, {count}/{} jobs completed", args.jobs);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    let mut report = ScenarioReport::new(
        "throughput",
        "Add jobs due now from concurrent producers, measure submission, dispatch and completion rates. \
         Completion is polled every 100ms.",
    );
    report.latencies = vec![add_rpc.summary(), submit_to_ready.summary()];
    report.metrics = vec![
        Metric::duration("submit time", submit_time),
        Metric::rate("submit rate", args.jobs, submit_time),
        Metric::duration("dispatch time", dispatch_time),
        Metric::rate("dispatch rate", args.jobs, dispatch_time),
        Metric::duration("completion time", completion_time),
        Metric::rate("completion rate", args.jobs, completion_time),
    ];
    Ok(report)
}
