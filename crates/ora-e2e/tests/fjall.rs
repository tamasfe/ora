#![allow(missing_docs)]

use ora_e2e::{tests::smoke, util::init_test};
use ora_storage_fjall::{FjallStorage, FjallStorageConfig};

fn persistent_storage_factory() -> impl Fn() -> FjallStorage {
    std::fs::create_dir_all(".local.test_data").unwrap();

    let dir = tempfile::TempDir::new_in(".local.test_data").unwrap();

    move || {
        FjallStorage::new(
            FjallStorageConfig::new(fjall::Config::new(dir.path())).transcation_durability(None),
        )
        .unwrap()
    }
}

fn new_storage_factory() -> impl Fn() -> FjallStorage {
    std::fs::create_dir_all(".local.test_data").unwrap();

    move || {
        FjallStorage::new(
            FjallStorageConfig::new(fjall::Config::new(
                tempfile::TempDir::new_in(".local.test_data")
                    .unwrap()
                    .into_path(),
            ))
            .transcation_durability(None),
        )
        .unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_wait_job() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_wait_job_multiple() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job_multiple(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_wait_job_multiple_concurrent() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job_multiple_concurrent(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_job_events() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_events(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_job_timeout() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_timeout(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_job_panic() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_panic(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_job_retry() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_retry(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_no_executors() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_no_executors(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_job_timing() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_timing(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_executor_disconnect() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_executor_disconnect(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_server_shutdown() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_server_shutdown(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_storage_queries() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::queries::test_storage_queries(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_job() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::cancellation::test_cancel_job(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_job_labels() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::queries::test_job_labels(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_schedule() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::schedules::test_schedule(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_schedule_timing() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::schedules::test_schedule_timing(persistent_storage_factory()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_snapshots() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::snapshots::test_snapshots(new_storage_factory()).await
}
