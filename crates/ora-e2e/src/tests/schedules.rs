use std::time::{Duration, SystemTime};

use futures::TryStreamExt;
use ora::{executor::IntoExecutionHandler, job_type::JobTypeExt, AdminClient};
use ora_server::{ServerOptions, Storage, TimerOptions};
use tokio::time::sleep;

use crate::{jobs, util::log_audit_events};

/// A simple test that schedules jobs.
pub async fn test_schedule<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            executor_shutdown_timeout: Duration::from_secs(1),
            executor_heartbeat_timeout: Duration::from_secs(1),
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

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let schedule = client
        .add_schedule(
            jobs::WaitForever
                .job()
                .repeat_every(Duration::from_millis(10))
                .immediate(),
        )
        .await?;

    sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(schedule.job_count().await?, 1);

    let first_job = schedule.active_job().await?.unwrap();

    assert!(first_job.status().await?.is_running());

    first_job.cancel().await?;

    assert!(first_job.status().await?.is_failed());

    sleep(std::time::Duration::from_millis(1000)).await;

    let second_job = schedule.active_job().await?.unwrap();

    assert!(second_job.status().await?.is_running());

    assert_eq!(schedule.job_count().await?, 2);

    schedule.cancel(true).await?;

    sleep(Duration::from_millis(200)).await;

    assert_eq!(schedule.job_count().await?, 2);
    assert!(schedule.active_job().await?.is_none());
    assert!(second_job.status().await?.is_failed());

    server.shutdown().await?;

    Ok(())
}

/// A test that schedules jobs and checks their timing.
pub async fn test_schedule_timing<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: std::time::Duration::from_millis(100),
            executor_shutdown_timeout: Duration::from_secs(1),
            executor_heartbeat_timeout: Duration::from_secs(1),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;
    crate::util::log_audit_events(&server);

    let mut executor = ora::Executor::new(server.executor_service_client());
    executor.add_handler(jobs::assert_execution_time_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let target_start = SystemTime::now() + Duration::from_millis(1000);
    let target_end = target_start + Duration::from_millis(1000);

    let schedule = client
        .add_schedule(
            jobs::AssertExecutionTime {
                after: target_start,
                before: target_end,
            }
            .job()
            .repeat_every(Duration::from_millis(50))
            .immediate()
            .start_after(target_start)
            .end_before(target_end),
        )
        .await?;

    sleep(std::time::Duration::from_millis(200)).await;

    assert!(schedule.details().await?.active);

    sleep(std::time::Duration::from_millis(3000)).await;

    let mut schedule_jobs = schedule.jobs(Default::default(), Default::default());

    while let Some(job) = schedule_jobs.try_next().await? {
        assert!(job.status().await?.is_succeeded());
    }

    assert!(schedule.job_count().await? >= 20);
    assert!(!schedule.details().await?.active);

    server.shutdown().await?;

    Ok(())
}
