#![allow(missing_docs)]
use ora_e2e::{tests::smoke, util::init_test};
use ora_storage_memory::MemoryStorage;

#[tokio::test]
async fn test_wait_job() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job(MemoryStorage::new).await
}

#[tokio::test]
async fn test_wait_job_multiple() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job_multiple(MemoryStorage::new).await
}

#[tokio::test]
async fn test_wait_job_multiple_concurrent() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job_multiple_concurrent(MemoryStorage::new).await
}

#[tokio::test]
async fn test_job_events() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_events(MemoryStorage::new).await
}

#[tokio::test]
async fn test_job_timeout() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_timeout(MemoryStorage::new).await
}

#[tokio::test]
async fn test_job_panic() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_panic(MemoryStorage::new).await
}

#[tokio::test]
async fn test_job_retry() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_retry(MemoryStorage::new).await
}

#[tokio::test]
async fn test_no_executors() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_no_executors(MemoryStorage::new).await
}

#[tokio::test]
async fn test_job_timing() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_timing(MemoryStorage::new).await
}

#[tokio::test]
async fn test_executor_disconnect() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_executor_disconnect(MemoryStorage::new).await
}

#[tokio::test]
async fn test_server_shutdown() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_server_shutdown(MemoryStorage::new).await
}

#[tokio::test]
async fn test_storage_queries() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::queries::test_storage_queries(MemoryStorage::new).await
}

#[tokio::test]
async fn test_cancel_job() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::cancellation::test_cancel_job(MemoryStorage::new).await
}

#[tokio::test]
async fn test_job_labels() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::queries::test_job_labels(MemoryStorage::new).await
}

#[tokio::test]
async fn test_schedule() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::schedules::test_schedule(MemoryStorage::new).await
}

#[tokio::test]
async fn test_schedule_timing() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::schedules::test_schedule_timing(MemoryStorage::new).await
}

#[tokio::test]
async fn test_snapshots() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::snapshots::test_snapshots(MemoryStorage::new).await
}
