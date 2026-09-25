//! End-to-end tests of an in-process server and executor with the Postgres backend.
#![cfg(all(feature = "server", feature = "executor"))]

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use deadpool_postgres::{
    Config, ManagerConfig, Pool, RecyclingMethod, Runtime, tokio_postgres::NoTls,
};
use ora::{
    AdminClient, IntoJob, JobType,
    execution::ExecutionStatus,
    executor::{Executor, HandlerOptions},
    job::TimeoutBaseTime,
    server::ServerHandleExt,
};
use ora_backend_postgres::PostgresBackend;
use ora_server::{Backend, ServerBuilder, ServerOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

/// The database to run the tests in, its `ora` schema is dropped by each test.
fn database_url() -> String {
    std::env::var("ORA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/postgres".to_string())
}

/// Run until cancelled, or return immediately.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "test")]
struct Hang {
    hang: bool,
}

/// Panic in the handler.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "test")]
struct Panic {}

/// Fail with a message containing a NUL character.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "test")]
struct FailWithNul {}

/// Sleep for the given duration.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "test")]
struct Sleep {
    millis: u64,
}

static HANG_CANCELLED: AtomicBool = AtomicBool::new(false);

fn pool() -> Pool {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn end_to_end() {
    let pool = pool();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    let grace_period = Duration::from_secs(10);

    let server = ServerBuilder::new(
        PostgresBackend::new(pool).await.unwrap(),
        ServerOptions {
            shutdown_grace_period: grace_period,
            ..Default::default()
        },
    )
    .spawn();

    let admin =
        AdminClient::new(server.admin_client()).with_poll_interval(Duration::from_millis(50));

    let _executor = Executor::new(server.execution_client())
        .handler(async |ctx, job: Hang| {
            if job.hang {
                ctx.cancelled().await;
                HANG_CANCELLED.store(true, Ordering::SeqCst);
            }
            Ok(())
        })
        .handler(async |_, _: Panic| -> ora::executor::HandlerResult<()> {
            panic!("handler panic");
        })
        .handler(
            async |_, _: FailWithNul| -> ora::executor::HandlerResult<()> {
                Err(ora::executor::HandlerError::msg("before\0after"))
            },
        )
        .handler_with_options(
            async |_, job: Sleep| {
                tokio::time::sleep(Duration::from_millis(job.millis)).await;
                Ok(())
            },
            HandlerOptions::default().with_max_concurrent(4),
        )
        .spawn();

    // Timed out executions are cancelled on the executor and free its capacity.
    {
        let mut job = admin
            .add_job(
                Hang { hang: true }
                    .now()
                    .with_timeout(Duration::from_secs(1), TimeoutBaseTime::StartTime),
            )
            .await
            .unwrap();

        timeout(Duration::from_secs(20), job.terminated())
            .await
            .unwrap()
            .unwrap();

        let failure_reason = job.failure_reason().await.unwrap().unwrap();
        assert!(failure_reason.contains("timed out"), "{failure_reason}");

        timeout(Duration::from_secs(10), async {
            while !HANG_CANCELLED.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the timed out execution was not cancelled");

        // The queue only allows a single execution.
        let mut job = admin.add_job(Hang { hang: false }.now()).await.unwrap();

        timeout(Duration::from_secs(20), job.terminated())
            .await
            .expect("the timed out execution still occupies the queue")
            .unwrap();
        assert!(job.output_json().await.unwrap().is_some());
    }

    // Panicking handlers are reported as failures and free their capacity.
    for _ in 0..2 {
        let mut job = admin.add_job(Panic {}.now()).await.unwrap();

        timeout(Duration::from_secs(20), job.terminated())
            .await
            .expect("the panicking execution was not reported")
            .unwrap();

        let failure_reason = job.failure_reason().await.unwrap().unwrap();
        assert!(failure_reason.contains("handler panic"), "{failure_reason}");
    }

    // Failure reasons the backend cannot store as-is
    // do not block processing other results.
    {
        let mut job = admin.add_job(FailWithNul {}.now()).await.unwrap();

        timeout(Duration::from_secs(20), job.terminated())
            .await
            .expect("the failed execution was not recorded")
            .unwrap();

        let failure_reason = job.failure_reason().await.unwrap().unwrap();
        assert!(failure_reason.contains("before"), "{failure_reason}");

        let mut job = admin.add_job(Hang { hang: false }.now()).await.unwrap();

        timeout(Duration::from_secs(20), job.terminated())
            .await
            .expect("results are not processed anymore")
            .unwrap();
        assert!(job.output_json().await.unwrap().is_some());
    }

    // Results reported within the shutdown grace period are kept.
    {
        let mut job = admin.add_job(Sleep { millis: 2000 }.now()).await.unwrap();
        let job_id = job.id();

        timeout(Duration::from_secs(20), async {
            loop {
                let executions = job.executions().await.unwrap();

                if executions
                    .last()
                    .is_some_and(|e| e.status() == ExecutionStatus::InProgress)
                {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();

        let started = Instant::now();
        timeout(grace_period * 2, server.stop())
            .await
            .expect("server did not stop");

        // Not waiting for the whole grace period after the execution finished.
        assert!(started.elapsed() < grace_period, "{:?}", started.elapsed());

        let backend = PostgresBackend::new(self::pool()).await.unwrap();

        let (jobs, _) = backend
            .list_jobs(
                ora_backend::jobs::JobFilters {
                    job_ids: Some(vec![ora_backend::jobs::JobId(job_id.0)]),
                    ..Default::default()
                },
                None,
                10,
                None,
            )
            .await
            .unwrap();

        let executions = &jobs[0].executions;
        assert_eq!(executions.len(), 1);
        assert_eq!(
            executions[0].status,
            ora_backend::executions::ExecutionStatus::Succeeded
        );
    }
}
