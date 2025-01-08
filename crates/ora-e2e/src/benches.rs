use std::{
    sync::{atomic::AtomicU64, Arc},
    time::{Duration, SystemTime},
};

use ora_client::{
    executor::{ExecutionContext, IntoExecutionHandler},
    job_type::JobTypeExt,
    AdminClient, JobType,
};
use ora_server::{ServerOptions, Storage};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::util::{init_tracing, log_audit_events};

#[allow(clippy::cast_possible_truncation)]
pub async fn bench_generic<S: Storage>(
    storage: S,
    server_options: ServerOptions,
    job_count: usize,
    bench_time: Duration,
    steps: usize,
) -> eyre::Result<GenericBenchResult> {
    let server = ora_server::Server::spawn(storage, server_options)?;
    init_tracing();
    log_audit_events(&server);

    let state = Arc::new(BenchState::default());

    let executor_count = 10;

    for _ in 0..executor_count {
        let mut executor = ora_client::Executor::new(server.executor_service_client());
        executor.add_handler({
            let state = state.clone();

            (move |ctx: ExecutionContext, _: MeasureLatencyJob| {
                let state = state.clone();

                async move {
                    let latency = SystemTime::now()
                        .duration_since(ctx.target_execution_time())
                        .unwrap();

                    state.total_ns.fetch_add(
                        latency.as_nanos() as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    state
                        .count
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    state.min_ns.fetch_min(
                        latency.as_nanos() as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    state.max_ns.fetch_max(
                        latency.as_nanos() as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );

                    Ok(())
                }
            })
            .handler()
        });

        tokio::spawn(async move {
            executor.run().await.unwrap();
        });
    }

    let client = AdminClient::new(server.admin_service_client());

    let step_wait = bench_time / steps as u32;
    let batch_size = job_count / steps;

    for _ in 0..steps {
        let target = SystemTime::now();

        let jobs = (0..batch_size)
            .map(|_| MeasureLatencyJob.job().at(target))
            .collect::<Vec<_>>();

        client.add_jobs(jobs).await?;

        tokio::time::sleep(step_wait).await;
    }

    let total_ns = state.total_ns.load(std::sync::atomic::Ordering::Relaxed);
    let count = state.count.load(std::sync::atomic::Ordering::Relaxed);

    let min_ns = state.min_ns.load(std::sync::atomic::Ordering::Relaxed);
    let max_ns = state.max_ns.load(std::sync::atomic::Ordering::Relaxed);

    if count == 0 {
        return Ok(GenericBenchResult {
            min: Duration::ZERO,
            max: Duration::ZERO,
            mean: Duration::ZERO,
            count: 0,
        });
    }

    let min = Duration::from_nanos(min_ns);
    let max = Duration::from_nanos(max_ns);
    let mean = Duration::from_nanos(total_ns / count);

    Ok(GenericBenchResult {
        min,
        max,
        mean,
        count,
    })
}

#[derive(Debug)]
pub struct GenericBenchResult {
    pub min: Duration,
    pub max: Duration,
    pub mean: Duration,
    pub count: u64,
}

#[derive(Debug)]
struct BenchState {
    total_ns: AtomicU64,
    count: AtomicU64,
    min_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl Default for BenchState {
    fn default() -> Self {
        Self {
            total_ns: Default::default(),
            count: Default::default(),
            min_ns: AtomicU64::new(u64::MAX),
            max_ns: Default::default(),
        }
    }
}

#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
struct MeasureLatencyJob;
