#![allow(missing_docs)]
use std::{process::exit, time::Duration};

use ora_e2e::{benches::GenericBenchResult, util::monitor_deadlocks};
use ora_server::ServerOptions;
use ora_storage_fjall::{FjallStorage, FjallStorageConfig};
use ora_storage_memory::MemoryStorage;
use ora_storage_sqlite::{SqliteStorage, SqliteStorageConfig};

fn main() {
    let job_counts = vec![5000];
    let bench_times = vec![Duration::from_secs(5)];
    let steps = vec![10];

    let server_options = ServerOptions::default();

    monitor_deadlocks();

    for job_count in job_counts {
        for bench_time in &bench_times {
            for &steps in &steps {
                println!("running job_count: {job_count}, time: {bench_time:?}, steps: {steps}");
                print_result(
                    "sqlite",
                    job_count,
                    *bench_time,
                    bench_sqlite_storage(server_options.clone(), job_count, *bench_time, steps),
                );

                print_result(
                    "fjall",
                    job_count,
                    *bench_time,
                    bench_fjall_storage(server_options.clone(), job_count, *bench_time, steps),
                );

                print_result(
                    "memory",
                    job_count,
                    *bench_time,
                    bench_memory_storage(server_options.clone(), job_count, *bench_time, steps),
                );

                print_result(
                    "sqlite_memory",
                    job_count,
                    *bench_time,
                    bench_sqlite_storage_memory(
                        server_options.clone(),
                        job_count,
                        *bench_time,
                        steps,
                    ),
                );
            }
        }
    }

    exit(0);
}

#[tokio::main]
async fn bench_memory_storage(
    server_options: ora_server::ServerOptions,
    job_count: usize,
    bench_time: Duration,
    steps: usize,
) -> GenericBenchResult {
    ora_e2e::benches::bench_generic(
        MemoryStorage::default(),
        server_options,
        job_count,
        bench_time,
        steps,
    )
    .await
    .unwrap()
}

#[tokio::main]
async fn bench_fjall_storage(
    server_options: ora_server::ServerOptions,
    job_count: usize,
    bench_time: Duration,
    steps: usize,
) -> GenericBenchResult {
    std::fs::create_dir_all(".local.test_data").unwrap();
    let dir = tempfile::TempDir::new_in(".local.test_data").unwrap();

    ora_e2e::benches::bench_generic(
        FjallStorage::new(FjallStorageConfig::new(fjall::Config::new(dir.path()))).unwrap(),
        server_options,
        job_count,
        bench_time,
        steps,
    )
    .await
    .unwrap()
}

#[tokio::main]
async fn bench_sqlite_storage_memory(
    server_options: ora_server::ServerOptions,
    job_count: usize,
    bench_time: Duration,
    steps: usize,
) -> GenericBenchResult {
    ora_e2e::benches::bench_generic(
        SqliteStorage::new(SqliteStorageConfig::new_in_memory()).unwrap(),
        server_options,
        job_count,
        bench_time,
        steps,
    )
    .await
    .unwrap()
}

#[tokio::main]
async fn bench_sqlite_storage(
    server_options: ora_server::ServerOptions,
    job_count: usize,
    bench_time: Duration,
    steps: usize,
) -> GenericBenchResult {
    std::fs::create_dir_all(".local.test_data").unwrap();
    let dir = tempfile::TempDir::new_in(".local.test_data").unwrap();

    ora_e2e::benches::bench_generic(
        SqliteStorage::new(
            SqliteStorageConfig::new(dir.path().join("app.db"))
                .with_init(|conn| {
                    conn.execute_batch(
                        r#"--sql
                    PRAGMA main.page_size = 4096;
                    PRAGMA main.cache_size=10000;
                    PRAGMA main.synchronous=NORMAL;
                    PRAGMA main.journal_mode=WAL;
                    PRAGMA main.cache_size=5000;
                    PRAGMA main.temp_store = MEMORY;
                    "#,
                    )?;
                    Ok(())
                })
                .with_connection_count(4),
        )
        .unwrap(),
        server_options,
        job_count,
        bench_time,
        steps,
    )
    .await
    .unwrap()
}

fn print_result(
    name: &str,
    job_count: usize,
    bench_duration: Duration,
    result: GenericBenchResult,
) {
    println!("{name} ({job_count}, {bench_duration:?}): {result:#?}");
}
