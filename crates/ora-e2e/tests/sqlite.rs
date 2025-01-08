#![allow(missing_docs)]
use ora_e2e::{tests::smoke, util::init_test};
use ora_storage_sqlite::SqliteStorage;

fn temp_storage_factory() -> impl Fn() -> SqliteStorage {
    move || SqliteStorage::new(rusqlite::Connection::open_in_memory().unwrap()).unwrap()
}

#[tokio::test]
async fn test_wait_job() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job(temp_storage_factory()).await
}

#[tokio::test]
async fn test_wait_job_multiple() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job_multiple(temp_storage_factory()).await
}

#[tokio::test]
async fn test_wait_job_multiple_concurrent() -> eyre::Result<()> {
    init_test();
    smoke::test_wait_job_multiple_concurrent(temp_storage_factory()).await
}

#[tokio::test]
async fn test_job_events() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_events(temp_storage_factory()).await
}

#[tokio::test]
async fn test_job_timeout() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_timeout(temp_storage_factory()).await
}

#[tokio::test]
async fn test_job_panic() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_panic(temp_storage_factory()).await
}

#[tokio::test]
async fn test_job_retry() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_retry(temp_storage_factory()).await
}

#[tokio::test]
async fn test_no_executors() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_no_executors(temp_storage_factory()).await
}

#[tokio::test]
async fn test_job_timing() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_job_timing(temp_storage_factory()).await
}

#[tokio::test]
async fn test_executor_disconnect() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_executor_disconnect(temp_storage_factory()).await
}

#[tokio::test]
async fn test_server_shutdown() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::smoke::test_server_shutdown(temp_storage_factory()).await
}

#[tokio::test]
async fn test_storage_queries() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::queries::test_storage_queries(temp_storage_factory()).await
}

#[tokio::test]
async fn test_cancel_job() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::cancellation::test_cancel_job(temp_storage_factory()).await
}

#[tokio::test]
async fn test_job_labels() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::queries::test_job_labels(temp_storage_factory()).await
}

#[tokio::test]
async fn test_schedule() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::schedules::test_schedule(temp_storage_factory()).await
}

#[tokio::test]
async fn test_schedule_timing() -> eyre::Result<()> {
    init_test();
    ora_e2e::tests::schedules::test_schedule_timing(temp_storage_factory()).await
}

// TODO: implement snapshots
// #[tokio::test]
// async fn test_snapshots() -> eyre::Result<()> {
//     init_test();
//     ora_e2e::tests::snapshots::test_snapshots(persistent_storage_factory()).await
// }
