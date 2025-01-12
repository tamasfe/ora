//! Test snapshot export and import.

use std::time::Duration;

use ora::{
    executor::IntoExecutionHandler, job_type::JobTypeExt, snapshot::SnapshotClient, AdminClient,
};
use ora_server::{ServerOptions, Storage, StorageSnapshot, TimerOptions};

use crate::jobs::{self, StrLen};

/// A test that creates a snapshot after a job is executed and then imports it.
pub async fn test_snapshots<S, F>(storage_factory: F) -> eyre::Result<()>
where
    S: Storage + StorageSnapshot,
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

    let len = job.clone().await?;

    assert_eq!(len, 5);

    let job_count = client.job_count(Default::default()).await?;

    let snapshot_client = SnapshotClient::new(server.snapshot_service_client());

    let snapshot_bytes = snapshot_client.export_to_bytes().await?;

    server.shutdown().await?;

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

    let snapshot_client = SnapshotClient::new(server.snapshot_service_client());
    snapshot_client.import_from_bytes(snapshot_bytes).await?;

    let client = AdminClient::new(server.admin_service_client());

    let job_count_after_import = client.job_count(Default::default()).await?;

    assert_eq!(job_count, job_count_after_import);

    let job = client.job(job.id());

    let len = job.cast_type::<StrLen>().await?;

    assert_eq!(len, 5);

    server.shutdown().await?;

    Ok(())
}
