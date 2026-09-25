//! End-to-end tests of executor backpressure and job priorities
//! with an in-process server and the Postgres backend.
#![cfg(all(feature = "server", feature = "executor"))]

use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use deadpool_postgres::{
    Config, ManagerConfig, Pool, RecyclingMethod, Runtime, tokio_postgres::NoTls,
};
use futures::StreamExt;
use ora::{
    AdminClient, IntoJob, JobType,
    execution::ExecutionStatus,
    executor::{Admission, Executor, HandlerOptions},
    proto::{
        executors::v1::{
            ExecutionSucceeded, ExecutorCapabilities, ExecutorConnectionRequest, ExecutorHeartbeat,
            ExecutorJobQueue, ExecutorMessage, executor_message::ExecutorMessageKind,
            server_message::ServerMessageKind,
        },
        jobs::v1::JobType as ProtoJobType,
    },
    server::ServerHandleExt,
};
use ora_backend_postgres::PostgresBackend;
use ora_server::{ServerBuilder, ServerOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

/// The database to run the tests in, its `ora` schema is dropped by each test.
fn database_url() -> String {
    std::env::var("ORA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/postgres".to_string())
}

fn pool() -> Pool {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap()
}

/// Returns immediately, gated by the executor's execution guard.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "test")]
struct Gated {}

/// Records the order of executions, gated by the handler's execution guard.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "test")]
struct Record {
    id: u32,
}

/// Handled by an executor that does not explicitly accept executions.
#[derive(Debug, JobType, Serialize, Deserialize, JsonSchema)]
#[ora(namespace = "test")]
struct Legacy {}

static EXECUTOR_READY: AtomicBool = AtomicBool::new(false);
static EXECUTOR_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static RECORD_READY: AtomicBool = AtomicBool::new(false);
static RECORDED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

#[tokio::test(flavor = "multi_thread")]
async fn backpressure_and_priorities() {
    let pool = pool();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    let server = ServerBuilder::new(
        PostgresBackend::new(pool).await.unwrap(),
        ServerOptions::default(),
    )
    .spawn();

    let admin =
        AdminClient::new(server.admin_client()).with_poll_interval(Duration::from_millis(50));

    let _executor = Executor::new(server.execution_client())
        .with_execution_guard(async |ctx| {
            if ctx.job_type_id().as_str() == Gated::job_type_id().as_str()
                && !EXECUTOR_READY.load(Ordering::SeqCst)
            {
                EXECUTOR_REJECTIONS.fetch_add(1, Ordering::SeqCst);
                return Admission::reject_with("executor is not ready");
            }

            Admission::Accept
        })
        .handler(async |_, _: Gated| Ok(()))
        .handler_with_options(
            async |_, job: Record| {
                RECORDED.lock().unwrap().push(job.id);
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(())
            },
            HandlerOptions::default()
                .with_max_concurrent(1)
                // Rejected without a reason.
                .with_execution_guard(async |_| {
                    Admission::accept_if(RECORD_READY.load(Ordering::SeqCst))
                }),
        )
        .spawn();

    // Rejected executions are not assigned, and are executed once the executor is ready.
    {
        let mut job = admin.add_job(Gated {}.now()).await.unwrap();

        timeout(Duration::from_secs(20), async {
            while EXECUTOR_REJECTIONS.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the execution was not offered to the executor");

        let raw = job.raw().await.unwrap();
        assert_eq!(raw.executions.len(), 1, "rejections must not be attempts");
        assert_eq!(
            job.executions().await.unwrap()[0].status(),
            ExecutionStatus::Pending
        );
        assert!(raw.executions[0].executor_id.is_none());
        assert!(raw.executions[0].started_at.is_none());

        EXECUTOR_READY.store(true, Ordering::SeqCst);

        timeout(Duration::from_secs(40), job.terminated())
            .await
            .expect("the execution was not accepted after the executor became ready")
            .unwrap();

        let executions = job.executions().await.unwrap();
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].status(), ExecutionStatus::Succeeded);

        // The execution started (when it was offered) before the executor reported its result.
        let raw = job.raw().await.unwrap();
        let started_at = SystemTime::try_from(raw.executions[0].started_at.unwrap()).unwrap();
        let succeeded_at = SystemTime::try_from(raw.executions[0].succeeded_at.unwrap()).unwrap();
        assert!(
            started_at <= succeeded_at,
            "{started_at:?} > {succeeded_at:?}"
        );
    }

    // Jobs with higher priority are executed first when capacity is limited.
    {
        let low = (0..4).map(|id| Record { id }.now());
        let high = (10..14).map(|id| Record { id }.now().with_priority(10));

        // Lower priority jobs are added first, so they are older.
        let mut jobs = admin.add_jobs(low).await.unwrap();
        jobs.extend(admin.add_jobs(high).await.unwrap());

        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(RECORDED.lock().unwrap().is_empty());

        RECORD_READY.store(true, Ordering::SeqCst);

        for job in &mut jobs {
            timeout(Duration::from_secs(40), job.terminated())
                .await
                .expect("the execution did not finish")
                .unwrap();
        }

        let recorded = RECORDED.lock().unwrap().clone();
        assert_eq!(recorded.len(), 8);
        assert!(
            recorded[..4].iter().all(|id| *id >= 10),
            "high priority jobs were not executed first: {recorded:?}"
        );
        assert_eq!(recorded[4..], [0, 1, 2, 3], "{recorded:?}");
    }

    // Executors that do not explicitly accept executions get them assigned right away.
    {
        let mut client = server.execution_client();

        let (outgoing, outgoing_recv) = futures::channel::mpsc::unbounded();

        outgoing
            .unbounded_send(ExecutorMessageKind::Capabilities(ExecutorCapabilities {
                name: "legacy".into(),
                job_queues: vec![ExecutorJobQueue {
                    job_type: Some(ProtoJobType {
                        id: Legacy::job_type_id().to_string(),
                        description: None,
                        input_schema_json: None,
                        output_schema_json: None,
                    }),
                    max_concurrent_executions: 1,
                }],
                execution_handshake: false,
            }))
            .unwrap();

        let mut incoming = client
            .executor_connection(outgoing_recv.map(|kind| ExecutorConnectionRequest {
                message: Some(ExecutorMessage {
                    executor_message_kind: Some(kind),
                }),
            }))
            .await
            .unwrap()
            .into_inner();

        let legacy_executor = tokio::spawn({
            let outgoing = outgoing.clone();

            async move {
                while let Ok(Some(message)) = incoming.message().await {
                    match message.message.and_then(|m| m.server_message_kind) {
                        Some(ServerMessageKind::ExecutionReady(ready)) => {
                            outgoing
                                .unbounded_send(ExecutorMessageKind::ExecutionSucceeded(
                                    ExecutionSucceeded {
                                        execution_id: ready.execution_id,
                                        timestamp: Some(SystemTime::now().into()),
                                        output_payload_json: "null".into(),
                                    },
                                ))
                                .unwrap();
                        }
                        Some(ServerMessageKind::Properties(_)) => {
                            _ = outgoing.unbounded_send(ExecutorMessageKind::Heartbeat(
                                ExecutorHeartbeat {},
                            ));
                        }
                        _ => {}
                    }
                }
            }
        });

        let mut job = admin.add_job(Legacy {}.now()).await.unwrap();

        timeout(Duration::from_secs(20), job.terminated())
            .await
            .expect("the legacy executor did not execute the job")
            .unwrap();

        let executions = job.executions().await.unwrap();
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].status(), ExecutionStatus::Succeeded);

        legacy_executor.abort();
    }
}
