//! Admin RPC response times.

use std::time::{Duration, Instant, SystemTime};

use futures::StreamExt;
use ora::proto::admin::v1::{
    CancelJobsRequest, CountJobsRequest, JobFilters, ListExecutorsRequest, ListJobTypesRequest,
    ListJobsRequest, PaginationOptions,
};
use serde::Serialize;

use crate::{
    executor::Mode,
    scenarios::{Ctx, Run, add_jobs},
    stats::{LatencySummary, Recorder, ScenarioReport},
};

/// Options for the `rpc` scenario.
#[derive(Debug, Clone, clap::Args, Serialize)]
pub struct RpcArgs {
    /// The number of requests per RPC.
    #[arg(long, default_value_t = 1000)]
    pub requests: usize,
    /// The number of concurrent requests.
    #[arg(long, default_value_t = 8)]
    pub concurrency: usize,
    /// The number of jobs in a batched `AddJobs` request.
    #[arg(long, default_value_t = 100)]
    pub batch_size: usize,
    /// The number of jobs to add before measuring
    /// so that list and count requests have data to work with.
    #[arg(long, default_value_t = 1000)]
    pub seed_jobs: usize,
}

impl Default for RpcArgs {
    fn default() -> Self {
        Self {
            requests: 1000,
            concurrency: 8,
            batch_size: 100,
            seed_jobs: 1000,
        }
    }
}

/// Measure the response times of admin RPCs.
///
/// All jobs are scheduled far in the future so that they are never executed.
pub async fn run(ctx: &mut Ctx, run: &Run, args: &RpcArgs) -> eyre::Result<ScenarioReport> {
    let far_future = SystemTime::now() + Duration::from_hours(24 * 30);
    let batch_size = args.batch_size.max(1);
    let total = ctx.warmup + args.requests;
    let admin = &ctx.admin;

    let add_far_future = |count: usize| {
        let mut admin = admin.clone();
        async move {
            let mut ids = Vec::with_capacity(count);
            while ids.len() < count {
                let n = batch_size.min(count - ids.len());
                let jobs = (0..n).map(|_| run.job(far_future, Mode::Succeed)).collect();
                ids.extend(add_jobs(&mut admin, jobs).await?);
            }
            eyre::Ok(ids)
        }
    };

    add_far_future(args.seed_jobs).await?;
    // Jobs that are cancelled one by one in the `CancelJobs` benchmark.
    let to_cancel = add_far_future(total).await?;

    let mut latencies = Vec::new();

    latencies.push(
        bench_rpc(
            "ListJobTypes".into(),
            0..total,
            args.concurrency,
            ctx.warmup,
            |_| {
                let mut admin = admin.clone();
                async move {
                    admin.list_job_types(ListJobTypesRequest {}).await?;
                    Ok(())
                }
            },
        )
        .await?,
    );

    latencies.push(
        bench_rpc(
            "ListExecutors".into(),
            0..total,
            args.concurrency,
            ctx.warmup,
            |_| {
                let mut admin = admin.clone();
                async move {
                    admin.list_executors(ListExecutorsRequest {}).await?;
                    Ok(())
                }
            },
        )
        .await?,
    );

    latencies.push(
        bench_rpc(
            "CountJobs".into(),
            0..total,
            args.concurrency,
            ctx.warmup,
            |_| {
                let mut admin = admin.clone();
                let filters = Some(run.filters());
                async move {
                    admin.count_jobs(CountJobsRequest { filters }).await?;
                    Ok(())
                }
            },
        )
        .await?,
    );

    latencies.push(
        bench_rpc(
            "ListJobs (50)".into(),
            0..total,
            args.concurrency,
            ctx.warmup,
            |_| {
                let mut admin = admin.clone();
                let filters = Some(run.filters());
                async move {
                    admin
                        .list_jobs(ListJobsRequest {
                            filters,
                            order_by: 0,
                            pagination: Some(PaginationOptions {
                                page_size: 50,
                                next_page_token: None,
                            }),
                        })
                        .await?;
                    Ok(())
                }
            },
        )
        .await?,
    );

    for size in [1, batch_size] {
        latencies.push(
            bench_rpc(
                format!("AddJobs ({size})"),
                0..total,
                args.concurrency,
                ctx.warmup,
                |_| {
                    let mut admin = admin.clone();
                    let jobs = (0..size)
                        .map(|_| run.job(far_future, Mode::Succeed))
                        .collect();
                    async move {
                        add_jobs(&mut admin, jobs).await?;
                        Ok(())
                    }
                },
            )
            .await?,
        );
    }

    latencies.push(
        bench_rpc(
            "CancelJobs (1)".into(),
            to_cancel,
            args.concurrency,
            ctx.warmup,
            |id| {
                let mut admin = admin.clone();
                async move {
                    admin
                        .cancel_jobs(CancelJobsRequest {
                            filters: Some(JobFilters {
                                job_ids: vec![id],
                                ..Default::default()
                            }),
                        })
                        .await?;
                    Ok(())
                }
            },
        )
        .await?,
    );

    let mut report = ScenarioReport::new(
        "rpc",
        "Admin RPC response times with concurrent clients. The rate column is requests per second.",
    );
    report.latencies = latencies;
    Ok(report)
}

/// Call `f` once for each input with the given concurrency
/// and record the response times.
async fn bench_rpc<T, F, Fut>(
    name: String,
    inputs: impl IntoIterator<Item = T>,
    concurrency: usize,
    warmup: usize,
    f: F,
) -> eyre::Result<LatencySummary>
where
    F: Fn(T) -> Fut,
    Fut: Future<Output = eyre::Result<()>>,
{
    let mut recorder = Recorder::new(name, warmup);
    let mut count = 0;

    let start = Instant::now();
    let mut results = futures::stream::iter(inputs)
        .map(|input| {
            let fut = f(input);
            async move {
                let t0 = Instant::now();
                fut.await.map(|()| t0.elapsed())
            }
        })
        .buffer_unordered(concurrency.max(1));

    while let Some(res) = results.next().await {
        recorder.record(res?);
        count += 1;
    }

    Ok(recorder.summary().with_throughput(count, start.elapsed()))
}
