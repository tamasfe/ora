//! Simple test cases.

use std::time::{Duration, SystemTime};

use futures::{stream::FuturesOrdered, StreamExt, TryStreamExt};
use ora::{
    executor::IntoExecutionHandler, job_definition::JobStatus, job_type::JobTypeExt, AdminClient,
};
use ora_server::{AuditEvent, AuditEventKind, ServerOptions, Storage, TimerOptions};
use tokio::time::sleep;

use crate::{jobs, util::log_audit_events};

/// A test that simply spawns a job and waits for it to complete.
pub async fn test_wait_job<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::str_len_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let job = client
        .add_job(
            jobs::StrLen {
                string: "hello".to_string(),
            }
            .job(),
        )
        .await?;

    let len = job.await?;

    assert_eq!(len, 5);

    server.shutdown().await?;

    Ok(())
}

/// A test that simply spawns jobs and waits for them to complete.
pub async fn test_wait_job_multiple<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::str_len_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    for i in 0..10 {
        let job = client
            .add_job(
                jobs::StrLen {
                    string: "a".repeat(i),
                }
                .job(),
            )
            .await?;

        let len = job.await?;

        assert_eq!(len, i);
    }

    server.shutdown().await?;

    Ok(())
}

/// A test that simply spawns jobs and waits for them to complete.
pub async fn test_wait_job_multiple_concurrent<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::str_len_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let mut jobs = Vec::new();

    for i in 0..100 {
        let job = client
            .add_job(
                jobs::StrLen {
                    string: "a".repeat(i),
                }
                .job(),
            )
            .await?;

        jobs.push(job);
    }

    let results = jobs
        .into_iter()
        .map(std::future::IntoFuture::into_future)
        .collect::<FuturesOrdered<_>>()
        .try_collect::<Vec<_>>()
        .await?;

    for (i, len) in results.into_iter().enumerate() {
        assert_eq!(len, i);
    }

    server.shutdown().await?;

    Ok(())
}

/// A test that simply spawns jobs and waits for them to complete.
pub async fn test_job_events<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::str_len_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let mut events = server.events();

    tokio::spawn(async move {
        let mut last_event: Option<AuditEvent> = None;

        while let Some(event) = events.next().await {
            if let Some(last_event) = &last_event {
                assert!(event.timestamp >= last_event.timestamp);

                match &last_event.kind {
                    AuditEventKind::JobAdded { .. } => {
                        assert!(matches!(event.kind, AuditEventKind::ExecutionAdded { .. }));
                    }
                    AuditEventKind::ExecutionAdded { .. } => {
                        assert!(matches!(event.kind, AuditEventKind::ExecutionReady { .. }));
                    }
                    AuditEventKind::ExecutionReady { .. } => {
                        assert!(matches!(
                            event.kind,
                            AuditEventKind::ExecutionAssigned { .. }
                        ));
                    }
                    AuditEventKind::ExecutionAssigned { .. } => {
                        assert!(matches!(
                            event.kind,
                            AuditEventKind::ExecutionStarted { .. }
                        ));
                    }
                    AuditEventKind::ExecutionStarted { .. } => {
                        assert!(matches!(
                            event.kind,
                            AuditEventKind::ExecutionSucceeded { .. }
                                | AuditEventKind::ExecutionFailed { .. }
                        ));
                    }
                    _ => {}
                }
            }

            last_event = Some(event);
        }
    });

    for i in 0..10 {
        let job = client
            .add_job(
                jobs::StrLen {
                    string: "a".repeat(i),
                }
                .job(),
            )
            .await?;

        let output = job.await?;
        assert_eq!(output, i);
    }

    server.shutdown().await?;

    Ok(())
}

/// A test that tests job timeouts.
pub async fn test_job_timeout<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;
    log_audit_events(&server);

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::timeout_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let job = client.add_job(jobs::Timeout { seconds: 1 }.job()).await?;

    assert!(job.await.is_ok());

    let job = client.add_job(jobs::Timeout { seconds: 3 }.job()).await?;

    assert!(job.await.is_err());

    server.shutdown().await?;

    Ok(())
}

/// A test that tests job panics.
pub async fn test_job_panic<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;
    log_audit_events(&server);

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::panic_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let job = client.add_job(jobs::Panic.job()).await?;

    assert!(job.await.is_err());

    server.shutdown().await?;

    Ok(())
}

/// A test that tests job retries.
pub async fn test_job_retry<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;
    log_audit_events(&server);

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::retry3_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let job = client
        .add_job(
            jobs::Retry3 {
                succeed_at_attempt: 3,
            }
            .job(),
        )
        .await?;

    assert!(job.clone().await.is_ok());
    assert_eq!(job.details().await.unwrap().executions.len(), 3);

    let job = client
        .add_job(
            jobs::Retry3 {
                succeed_at_attempt: 1,
            }
            .job(),
        )
        .await?;

    assert!(job.clone().await.is_ok());
    assert!(job.details().await.unwrap().executions.len() == 1);

    let job = client
        .add_job(
            jobs::Retry3 {
                succeed_at_attempt: 5,
            }
            .job(),
        )
        .await?;

    assert!(job.clone().await.is_err());
    assert!(job.details().await.unwrap().executions.len() == 4);

    server.shutdown().await?;

    Ok(())
}

/// A test that tests the case where no executors are available.
pub async fn test_no_executors<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;
    log_audit_events(&server);

    let client = AdminClient::new(server.admin_service_client());

    let job = client
        .add_job(
            jobs::StrLen {
                string: "hello".to_string(),
            }
            .job(),
        )
        .await?;

    sleep(std::time::Duration::from_millis(1000)).await;

    assert!(job.details().await.unwrap().active);
    assert!(!job.details().await.unwrap().status().is_terminal());
    assert!(job.details().await.unwrap().status() == JobStatus::Ready);

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::str_len_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    job.await?;

    server.shutdown().await?;

    Ok(())
}

/// A test that tests job timing.
pub async fn test_job_timing<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let target = SystemTime::now() + std::time::Duration::from_secs(2);

    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;
    log_audit_events(&server);

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::str_len_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let job = client
        .add_job(
            jobs::StrLen {
                string: "hello".to_string(),
            }
            .job()
            .at(target),
        )
        .await?;

    job.clone().await?;

    let details = job.details().await.unwrap();

    let succeeded_at = details.executions.last().unwrap().started_at.unwrap();

    assert!(succeeded_at >= target);
    assert!(succeeded_at < target + std::time::Duration::from_millis(100));

    server.shutdown().await?;

    Ok(())
}

/// Test executor disconnect.
pub async fn test_executor_disconnect<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            executor_heartbeat_timeout: std::time::Duration::from_millis(50),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;
    log_audit_events(&server);

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::wait_forever_handler.handler());

    let executor_handle = tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let job = client.add_job(jobs::WaitForever.job()).await?;

    sleep(std::time::Duration::from_millis(100)).await;

    let details = job.details().await?;
    assert_eq!(details.executions.len(), 1);
    assert_eq!(
        details.executions.last().unwrap().status,
        JobStatus::Running
    );

    executor_handle.abort();

    sleep(std::time::Duration::from_millis(500)).await;

    let details = job.details().await?;

    assert_eq!(details.executions.len(), 2);
    assert_eq!(details.executions.last().unwrap().status, JobStatus::Ready);

    server.shutdown().await?;

    Ok(())
}

/// Test server shutdown.
pub async fn test_server_shutdown<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let storage = storage_factory();

    let job_id;
    {
        let server = ora_server::Server::spawn(
            storage.clone(),
            ServerOptions {
                bookkeeping_interval: std::time::Duration::from_millis(100),
                executor_heartbeat_timeout: std::time::Duration::from_millis(50),
                executor_shutdown_timeout: std::time::Duration::from_millis(50),
                timer: TimerOptions {
                    sleep_threshold: Duration::ZERO,
                    bookkeeping_interval: Duration::ZERO,
                },
                ..ServerOptions::default()
            },
        )?;

        let mut executor = ora::Executor::new(server.executor_service_client());
        executor.add_handler(jobs::wait_forever_handler.handler());

        let executor_handle = tokio::spawn(async move {
            executor.run().await.unwrap();
        });

        let client = AdminClient::new(server.admin_service_client());

        let job = client.add_job(jobs::WaitForever.job()).await?;

        job_id = job.id();

        sleep(std::time::Duration::from_millis(100)).await;

        let details = job.details().await?;
        assert_eq!(details.executions.len(), 1);
        assert_eq!(
            details.executions.last().unwrap().status,
            JobStatus::Running
        );

        server.shutdown().await?;

        assert!(executor_handle.is_finished());
    }

    let server = ora_server::Server::spawn(
        storage.clone(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            executor_heartbeat_timeout: std::time::Duration::from_millis(50),
            executor_shutdown_timeout: std::time::Duration::from_millis(50),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;

    sleep(std::time::Duration::from_millis(200)).await;

    let client = AdminClient::new(server.admin_service_client());

    let job = client.job(job_id);

    let details = job.details().await?;

    assert_eq!(details.executions.len(), 2);
    assert_eq!(details.executions.last().unwrap().status, JobStatus::Ready);

    Ok(())
}
