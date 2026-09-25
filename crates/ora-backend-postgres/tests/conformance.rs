//! Conformance tests for the Postgres backend implementation.

use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
use tokio_postgres::NoTls;

/// The database to run the tests in, its `ora` schema is dropped by each test.
fn database_url() -> String {
    std::env::var("ORA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/postgres".to_string())
}

#[tokio::test]
async fn smoke() {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    ora_backend::test::smoke(
        &ora_backend_postgres::PostgresBackend::new(pool)
            .await
            .unwrap(),
    )
    .await;
}

#[tokio::test]
async fn queries() {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    ora_backend::test::job_queries(
        &ora_backend_postgres::PostgresBackend::new(pool)
            .await
            .unwrap(),
    )
    .await;
}

#[tokio::test]
async fn pagination_and_ordering() {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    ora_backend::test::pagination_and_ordering(
        &ora_backend_postgres::PostgresBackend::new(pool)
            .await
            .unwrap(),
    )
    .await;
}

#[tokio::test]
async fn schedules() {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    ora_backend::test::schedules(
        &ora_backend_postgres::PostgresBackend::new(pool)
            .await
            .unwrap(),
    )
    .await;
}

#[tokio::test]
async fn counts() {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    ora_backend::test::counts(
        &ora_backend_postgres::PostgresBackend::new(pool)
            .await
            .unwrap(),
    )
    .await;
}

#[tokio::test]
async fn edge_cases() {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    ora_backend::test::edge_cases(
        &ora_backend_postgres::PostgresBackend::new(pool)
            .await
            .unwrap(),
    )
    .await;
}

#[tokio::test]
async fn priorities() {
    let mut cfg = Config::new();
    cfg.url = Some(database_url());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls).unwrap();

    pool.get()
        .await
        .unwrap()
        .execute("DROP SCHEMA IF EXISTS ora CASCADE", &[])
        .await
        .unwrap();

    ora_backend::test::priorities(
        &ora_backend_postgres::PostgresBackend::new(pool)
            .await
            .unwrap(),
    )
    .await;
}
