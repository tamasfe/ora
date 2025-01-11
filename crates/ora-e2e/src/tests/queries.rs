//! Tests for running queries against storage backends.

use std::time::{Duration, SystemTime};

use futures::TryStreamExt;
use ora_client::{
    executor::{ExecutorOptions, IntoExecutionHandler},
    job_definition::JobStatus,
    job_query::{JobFilter, JobOrder},
    job_type::JobTypeExt,
    AdminClient,
};
use ora_server::{ServerOptions, Storage, TimerOptions};
use tokio::time::sleep;

use crate::jobs;

/// A test that runs a series of queries against the storage backend.
pub async fn test_storage_queries<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: Duration::from_millis(100),
            executor_shutdown_timeout: Duration::from_secs(1),
            executor_heartbeat_timeout: Duration::from_secs(1),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;

    let mut executor = ora_client::Executor::with_options(
        server.executor_service_client(),
        ExecutorOptions {
            max_concurrent_executions: u32::MAX.try_into().unwrap(),
            ..Default::default()
        },
    );
    executor.add_handler(jobs::str_len_handler.handler());
    executor.add_handler(jobs::wait_forever_handler.handler());
    executor.add_handler(jobs::retry3_handler.handler());

    tokio::spawn(async move {
        executor.run().await.unwrap();
    });

    let client = AdminClient::new(server.admin_service_client());

    let expected_succeeded_job_count = 10;
    let expected_running_job_count = 6;
    let expected_pending_job_count = 7;
    let expected_ready_job_count = 8;
    let expected_failed_job_count = 9;

    let expected_active_job_count =
        expected_running_job_count + expected_pending_job_count + expected_ready_job_count;

    let expected_inactive_job_count = expected_succeeded_job_count + expected_failed_job_count;

    let expected_total_job_count = expected_active_job_count + expected_inactive_job_count;

    // These will be executed immediately.
    for _ in 0..expected_succeeded_job_count {
        client
            .add_job(
                jobs::StrLen {
                    string: "hello".to_string(),
                }
                .job(),
            )
            .await?;
    }

    // These will be running forever.
    for _ in 0..expected_running_job_count {
        client.add_job(jobs::WaitForever.job()).await?;
    }

    // These will be pending for a very long time.
    // Using a different method just to test the interface.
    client
        .add_jobs((0..expected_pending_job_count).map(|i| {
            jobs::StrLen {
                string: format!("hello {i}"),
            }
            .job()
            .at(SystemTime::now() + Duration::from_secs(60 * 60 * 24 * 365))
        }))
        .await?;

    // These jobs will be failed.
    for _ in 0..expected_failed_job_count {
        client
            .add_job(
                jobs::Retry3 {
                    succeed_at_attempt: 5,
                }
                .job(),
            )
            .await?;
    }

    // These jobs will be ready to run, as there will be no executor to run them.
    for _ in 0..expected_ready_job_count {
        client.add_job(jobs::Add { a: 1, b: 2 }.job()).await?;
    }

    // Give some time for scheduling and execution.
    sleep(Duration::from_millis(1000)).await;

    let succeeded_job_count = client
        .job_count(JobFilter::new().include_succeeded())
        .await?;
    let running_job_count = client.job_count(JobFilter::new().include_running()).await?;
    let pending_job_count = client.job_count(JobFilter::new().include_pending()).await?;
    let ready_job_count = client.job_count(JobFilter::new().include_ready()).await?;
    let failed_job_count = client.job_count(JobFilter::new().include_failed()).await?;

    let total_job_count = client.job_count(JobFilter::new()).await?;
    let active_job_count = client.job_count(JobFilter::new().active_only()).await?;
    let inactive_job_count = client.job_count(JobFilter::new().inactive_only()).await?;

    assert_eq!(succeeded_job_count, expected_succeeded_job_count);
    assert_eq!(running_job_count, expected_running_job_count);
    assert_eq!(pending_job_count, expected_pending_job_count);
    assert_eq!(ready_job_count, expected_ready_job_count);
    assert_eq!(failed_job_count, expected_failed_job_count);

    assert_eq!(active_job_count, expected_active_job_count);
    assert_eq!(inactive_job_count, expected_inactive_job_count);
    assert_eq!(total_job_count, expected_total_job_count);

    let retry3_jobs = client
        .jobs_of_type::<jobs::Retry3>(JobFilter::default(), JobOrder::default())
        .try_collect::<Vec<_>>()
        .await?;

    assert_eq!(retry3_jobs.len() as u64, expected_failed_job_count);

    for job in retry3_jobs {
        for execution in &job.details_cached().unwrap().executions {
            assert_eq!(execution.status, JobStatus::Failed);
        }
    }

    server.shutdown().await?;

    Ok(())
}

/// Test job labels.
pub async fn test_job_labels<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage,
    F: Fn() -> S,
{
    let server = ora_server::Server::spawn(
        storage_factory(),
        ServerOptions {
            bookkeeping_interval: Duration::from_millis(100),
            executor_shutdown_timeout: Duration::from_secs(1),
            executor_heartbeat_timeout: Duration::from_secs(1),
            timer: TimerOptions {
                sleep_threshold: Duration::ZERO,
                bookkeeping_interval: Duration::ZERO,
            },
            ..ServerOptions::default()
        },
    )?;

    let client = AdminClient::new(server.admin_service_client());

    client
        .add_job(
            jobs::StrLen {
                string: "hello".to_string(),
            }
            .job()
            .with_label("test_label", "test_value"),
        )
        .await?;

    client
        .add_job(
            jobs::StrLen {
                string: "hello".to_string(),
            }
            .job()
            .with_label("test_label", "test_value"),
        )
        .await?;

    let job = client
        .add_job(
            jobs::StrLen {
                string: "hello".to_string(),
            }
            .job()
            .with_label("test_label", "test_value2"),
        )
        .await?;

    let job_details = client.job(job.id()).details().await?;

    assert_eq!(
        job_details.labels.get("test_label").map(String::as_str),
        Some("test_value2")
    );

    assert_eq!(
        client
            .job_count(JobFilter::default().with_label_value("test_label", "test_value"))
            .await?,
        2
    );

    assert_eq!(
        client
            .job_count(JobFilter::default().with_label_value("test_label", "test_value2"))
            .await?,
        1
    );

    assert_eq!(
        client
            .job_count(JobFilter::default().with_label("test_label"))
            .await?,
        3
    );

    server.shutdown().await?;

    Ok(())
}
