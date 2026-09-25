//! Benchmark suite for Ora servers.
//!
//! Talks to the server exclusively over its gRPC APIs,
//! the target server must expose both the admin and the execution services.

use std::{path::PathBuf, time::Duration};

use clap::{Parser, Subcommand};
use eyre::{Context, bail};
use ora::proto::admin::v1::{
    ListExecutorsRequest, ListJobTypesRequest, admin_service_client::AdminServiceClient,
};
use serde::Serialize;
use tonic::transport::Endpoint;
use tracing::Level;

use crate::{
    executor::JOB_TYPE_ID,
    scenarios::{
        Ctx, Run, all_bench_jobs,
        cancel::{self, CancelArgs},
        delayed::{self, DelayedArgs},
        latency::{self, LatencyArgs},
        retry::{self, RetryArgs},
        rpc::{self, RpcArgs},
        throughput::{self, ThroughputArgs},
    },
    stats::ScenarioReport,
};

mod executor;
mod scenarios;
mod stats;

/// Benchmark an Ora server over its gRPC APIs.
///
/// The benchmark connects its own executor for the `ora_bench.Job` job type,
/// the server must expose both `AdminService` and `ExecutionService`.
///
/// All durations are measured with the local monotonic clock,
/// except for the `delayed` scenario which compares target execution times
/// with the local wall clock, and thus includes any clock skew
/// between the benchmark and the server.
#[derive(Debug, Parser, Serialize)]
#[command(version)]
struct Cli {
    /// The URL of the Ora server.
    #[arg(long, env = "ORA_URL")]
    url: String,
    /// Write the full report as JSON to this file.
    #[arg(long)]
    json: Option<PathBuf>,
    /// The number of initial samples to discard in closed-loop measurements.
    #[arg(long, default_value_t = 20)]
    warmup: usize,
    /// The maximum number of concurrent executions of the benchmark executor.
    #[arg(long, default_value_t = 1024)]
    executor_capacity: u64,
    /// The scenario to run, all scenarios are run by default.
    #[command(subcommand)]
    scenario: Option<Scenario>,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
#[serde(rename_all = "snake_case")]
enum Scenario {
    /// Run all scenarios with default options.
    All,
    /// Admin RPC response times.
    Rpc(RpcArgs),
    /// Scheduling latency of jobs due now, one job at a time.
    Latency(LatencyArgs),
    /// Lateness of jobs scheduled in the future.
    Delayed(DelayedArgs),
    /// Submission, dispatch and completion rates under load.
    Throughput(ThroughputArgs),
    /// Latency of retrying failed executions.
    Retry(RetryArgs),
    /// Latency of cancelling in-progress executions.
    Cancel(CancelArgs),
}

#[derive(Serialize)]
struct Report<'a> {
    started_at: String,
    options: &'a Cli,
    scenarios: Vec<ScenarioReport>,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let cli = Cli::parse();
    let started_at = jiff::Timestamp::now().to_string();

    let channel = Endpoint::from_shared(cli.url.clone())?
        .tcp_nodelay(true)
        .connect()
        .await
        .wrap_err_with(|| format!("failed to connect to {}", cli.url))?;

    let (executor, events) = executor::connect(channel.clone(), cli.executor_capacity).await?;
    let mut ctx = Ctx {
        admin: AdminServiceClient::new(channel),
        events,
        warmup: cli.warmup,
    };

    wait_for_executor(&mut ctx, &executor.executor_id).await?;
    tracing::info!(executor_id = %executor.executor_id, "benchmark executor connected");

    // Jobs left over from interrupted runs would otherwise
    // keep the executor busy.
    let stale = ctx.cancel(all_bench_jobs()).await?;
    if stale > 0 {
        tracing::info!(count = stale, "cancelled leftover benchmark jobs");
    }

    let scenarios = match cli.scenario.clone().unwrap_or(Scenario::All) {
        Scenario::All => vec![
            Scenario::Rpc(RpcArgs::default()),
            Scenario::Latency(LatencyArgs::default()),
            Scenario::Delayed(DelayedArgs::default()),
            Scenario::Throughput(ThroughputArgs::default()),
            Scenario::Retry(RetryArgs::default()),
            Scenario::Cancel(CancelArgs::default()),
        ],
        scenario => vec![scenario],
    };

    let mut reports = Vec::new();

    for scenario in scenarios {
        let run = Run::new();
        ctx.drain_events();

        let result = match &scenario {
            Scenario::All => unreachable!(),
            Scenario::Rpc(args) => rpc::run(&mut ctx, &run, args).await,
            Scenario::Latency(args) => latency::run(&mut ctx, &run, args).await,
            Scenario::Delayed(args) => delayed::run(&mut ctx, &run, args).await,
            Scenario::Throughput(args) => throughput::run(&mut ctx, &run, args).await,
            Scenario::Retry(args) => retry::run(&mut ctx, &run, args).await,
            Scenario::Cancel(args) => cancel::run(&mut ctx, &run, args).await,
        };

        if let Err(error) = ctx.cancel(run.filters()).await {
            tracing::warn!(%error, "failed to clean up benchmark jobs");
        }

        let report = result?;
        report.print();
        reports.push(report);
    }

    executor.shutdown().await;

    if let Some(path) = &cli.json {
        let report = Report {
            started_at,
            options: &cli,
            scenarios: reports,
        };
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)
            .wrap_err_with(|| format!("failed to write {}", path.display()))?;
        tracing::info!(path = %path.display(), "report written");
    }

    Ok(())
}

/// Wait until the server lists the benchmark executor and its job type.
async fn wait_for_executor(ctx: &mut Ctx, executor_id: &str) -> eyre::Result<()> {
    for _ in 0..100 {
        let executors = ctx
            .admin
            .list_executors(ListExecutorsRequest {})
            .await?
            .into_inner()
            .executors;

        let job_types = ctx
            .admin
            .list_job_types(ListJobTypesRequest {})
            .await?
            .into_inner()
            .job_types;

        if executors.iter().any(|e| e.id == executor_id)
            && job_types.iter().any(|jt| jt.id == JOB_TYPE_ID)
        {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    bail!("the server did not register the benchmark executor in time")
}
