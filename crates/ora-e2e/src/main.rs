//! Ora E2E test suite.

use async_trait::async_trait;
use ora::{
    client::Client, IntoWorker, MemoryStore, MemoryStoreOptions, NewTask, ScheduleDefinition,
    SchedulePolicy, Scheduler, Task, TaskContext, Worker, WorkerPool,
};
use ora_client::Tasks;
use ora_common::task::TaskDefinition;
use ora_store_sqlx::{DbStore, DbStoreOptions};
use sqlx::PgPool;
use tasks::{LatencyTestTask, ScheduleTestTask};
use time::{Duration, OffsetDateTime};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

pub mod tasks;

struct TestWorker;

#[async_trait]
impl Worker<LatencyTestTask> for TestWorker {
    async fn run(&self, _ctx: TaskContext, task: LatencyTestTask) -> eyre::Result<Duration> {
        let now = OffsetDateTime::now_utc();
        Ok((now - task.target).abs())
    }
}

#[async_trait]
impl Worker<ScheduleTestTask> for TestWorker {
    async fn run(&self, _ctx: TaskContext, _task: ScheduleTestTask) -> eyre::Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .pretty()
        .with_max_level(tracing::Level::INFO)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive("info".parse().unwrap())
                .from_env()
                .unwrap(),
        )
        .init();

    test_in_memory().await;
    test_postgres().await;
}

async fn test_in_memory() {
    tracing::info!("testing in-memory store");

    let store = MemoryStore::new_with_options(MemoryStoreOptions {
        channel_capacity: 10_000,
    });

    let scheduler = Scheduler::new(store.clone());
    let scheduler_handle = tokio::spawn(async move {
        if let Err(error) = scheduler.run().await {
            panic!("scheduler exited unexpectedly: {error}");
        }
    });

    let mut pool = WorkerPool::new(store.clone());
    setup_worker_pool(&mut pool);

    let pool_handle = tokio::spawn(async move {
        if let Err(error) = pool.run().await {
            panic!("worker pool exited unexpectedly: {error}");
        }
    });

    test_latency(
        &store,
        Duration::milliseconds(500),
        5,
        10,
        Duration::milliseconds(1),
    )
    .await;

    test_latency(
        &store,
        Duration::milliseconds(500),
        5,
        1000,
        Duration::milliseconds(10),
    )
    .await;

    test_schedule(&store, Duration::seconds(2)).await;

    scheduler_handle.abort();
    pool_handle.abort();

    // Not measuring latency, does not matter.
    let target = OffsetDateTime::now_utc();
    let persistence_id = Uuid::new_v4();
    for _ in 0..10 {
        store
            .add_task(
                LatencyTestTask { target }
                    .task()
                    .with_label("persistent", persistence_id),
            )
            .await
            .unwrap();
    }

    let task_count = store.task_count(&Tasks::all()).await.unwrap();

    assert_eq!(store.task_count(&Tasks::all()).await.unwrap(), task_count);

    let persistent_tasks = store
        .tasks(&Tasks::all().with_label_value("persistent", persistence_id))
        .await
        .unwrap();

    assert_eq!(persistent_tasks.len(), 10);

    let scheduler = Scheduler::new(store.clone());
    let scheduler_handle = tokio::spawn(async move {
        if let Err(error) = scheduler.run().await {
            panic!("scheduler exited unexpectedly: {error}");
        }
    });

    let mut pool = WorkerPool::new(store.clone());
    setup_worker_pool(&mut pool);

    let pool_handle = tokio::spawn(async move {
        if let Err(error) = pool.run().await {
            panic!("worker pool exited unexpectedly: {error}");
        }
    });

    for task in persistent_tasks {
        assert!(task.await.is_ok());
    }

    scheduler_handle.abort();
    pool_handle.abort();
}

async fn test_postgres() {
    tracing::info!("testing local postgres store");
    let db = PgPool::connect("postgres://postgres:postgres@localhost/postgres")
        .await
        .unwrap();

    let store = DbStore::new_with_options(
        db,
        DbStoreOptions {
            poll_interval: Duration::milliseconds(200),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let scheduler = Scheduler::new(store.clone());
    let scheduler_handle = tokio::spawn(async move {
        if let Err(error) = scheduler.run().await {
            panic!("scheduler exited unexpectedly: {error}");
        }
    });

    let mut pool = WorkerPool::new(store.clone());
    setup_worker_pool(&mut pool);

    let pool_handle = tokio::spawn(async move {
        if let Err(error) = pool.run().await {
            panic!("worker pool exited unexpectedly: {error}");
        }
    });

    test_latency(
        &store,
        Duration::milliseconds(500),
        5,
        50,
        Duration::seconds(5),
    )
    .await;

    test_schedule(&store, Duration::seconds(2)).await;

    scheduler_handle.abort();
    pool_handle.abort();

    // Not measuring latency, does not matter.
    let target = OffsetDateTime::now_utc();
    let persistence_id = Uuid::new_v4();
    for _ in 0..10 {
        store
            .add_task(
                LatencyTestTask { target }
                    .task()
                    .with_label("persistent", persistence_id),
            )
            .await
            .unwrap();
    }

    let task_count = store.task_count(&Tasks::all()).await.unwrap();

    drop(store);

    let db = PgPool::connect("postgres://postgres:postgres@localhost/postgres")
        .await
        .unwrap();

    let store = DbStore::new_with_options(
        db,
        DbStoreOptions {
            poll_interval: Duration::milliseconds(500),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(store.task_count(&Tasks::all()).await.unwrap(), task_count);

    let persistent_tasks = store
        .tasks(&Tasks::all().with_label_value("persistent", persistence_id))
        .await
        .unwrap();

    assert_eq!(persistent_tasks.len(), 10);

    let scheduler = Scheduler::new(store.clone());
    let scheduler_handle = tokio::spawn(async move {
        if let Err(error) = scheduler.run().await {
            panic!("scheduler exited unexpectedly: {error}");
        }
    });

    let mut pool = WorkerPool::new(store.clone());
    setup_worker_pool(&mut pool);

    let pool_handle = tokio::spawn(async move {
        if let Err(error) = pool.run().await {
            panic!("worker pool exited unexpectedly: {error}");
        }
    });

    for task in persistent_tasks {
        assert!(task.await.is_ok());
    }

    scheduler_handle.abort();
    pool_handle.abort();
}

fn setup_worker_pool<C>(pool: &mut WorkerPool<C>) {
    pool.register_worker(TestWorker.worker::<LatencyTestTask>());
    pool.register_worker(TestWorker.worker::<ScheduleTestTask>());
}

async fn test_latency(
    store: &impl Client,
    interval: Duration,
    batch_count: usize,
    batch_task_count: usize,
    allowed_latency: Duration,
) {
    tracing::info!(%interval, %batch_task_count, %allowed_latency, "running latency test");
    let now = OffsetDateTime::now_utc();

    let mut tasks: Vec<TaskDefinition<LatencyTestTask>> = Vec::new();
    for i in 0..(batch_count as i32) {
        for _ in 0..batch_task_count {
            let target = now + interval * i;
            tasks.push(LatencyTestTask { target }.task().at(target));
        }
    }

    let tasks = store.add_tasks(tasks).await.unwrap();

    for (n, task) in tasks.into_iter().enumerate() {
        let task_id = task.id();
        let latency = task.await.unwrap();
        assert!(
            latency <= allowed_latency,
            "latency {latency} for task {n} ({task_id}) was above the allowed range"
        );
    }
    tracing::info!("latency test passed");
}

async fn test_schedule(store: &impl Client, interval: Duration) {
    tracing::info!(%interval, "running schedule test");
    let schedule = store
        .add_schedule(
            ScheduleDefinition::new(
                SchedulePolicy::repeat(interval),
                NewTask::repeat(ScheduleTestTask.task()),
            )
            .immediate(true),
        )
        .await
        .unwrap();

    let t = async {
        let mut last_task_id = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let current_task = schedule.active_task().await.unwrap().unwrap();
            assert!(last_task_id != Some(current_task.id()));
            last_task_id = Some(current_task.id());
            current_task.await.unwrap();
            tokio::time::sleep((interval).try_into().unwrap()).await;
        }
    };

    tokio::select! {
        _ = tokio::time::sleep((interval * 5_i32).try_into().unwrap()) => {}
        _ = t => {}
    }

    schedule.cancel().await.unwrap();
    assert!(!schedule.is_active().await.unwrap());
    assert!(schedule.active_task().await.unwrap().is_none())
}
