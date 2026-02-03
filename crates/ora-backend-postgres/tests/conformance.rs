//! Conformance tests for the Postgres backend implementation.

use deadpool_postgres::{Config, ManagerConfig, RecyclingMethod, Runtime};
use tokio_postgres::NoTls;

#[tokio::test]
async fn smoke() {
    let mut cfg = Config::new();
    cfg.url = Some("postgresql://postgres:postgres@localhost:5432/postgres".to_string());
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
    cfg.url = Some("postgresql://postgres:postgres@localhost:5432/postgres".to_string());
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
    cfg.url = Some("postgresql://postgres:postgres@localhost:5432/postgres".to_string());
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
    cfg.url = Some("postgresql://postgres:postgres@localhost:5432/postgres".to_string());
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
