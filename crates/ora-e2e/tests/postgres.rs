use ora::{Client, Scheduler, Task, Worker};
use ora_client::Tasks;
use ora_e2e::{
    setup_worker, tasks::LatencyTestTask, test_latency, test_schedule_cancellation, test_schedules,
};
use ora_store_sqlx::{DbStore, DbStoreOptions};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    Postgres,
};
use time::{Duration, OffsetDateTime};
use tokio::test;
use uuid::Uuid;

const CONCURRENT_WORKERS: usize = 30;

async fn db_store() -> DbStore<Postgres> {
    let options: PgConnectOptions = std::env::var("DATABASE_URL")
        .as_deref()
        .unwrap_or("postgres://postgres:postgres@localhost/postgres")
        .parse()
        .unwrap();

    let pool_options = PgPoolOptions::new().max_connections(50);

    let db = pool_options.connect_with(options).await.unwrap();

    DbStore::new_with_options(
        db,
        DbStoreOptions {
            poll_interval: Duration::milliseconds(200),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[test]
async fn latency() {
    let store = db_store().await;

    let scheduler = Scheduler::new(store.clone());
    let scheduler_handle = tokio::spawn(async move {
        if let Err(error) = scheduler.run().await {
            panic!("scheduler exited unexpectedly: {error}");
        }
    });

    let mut worker_handles = Vec::with_capacity(CONCURRENT_WORKERS);
    for _ in 0..CONCURRENT_WORKERS {
        let mut worker = Worker::new(store.clone());
        setup_worker(&mut worker);

        worker_handles.push(tokio::spawn(async move {
            if let Err(error) = worker.run().await {
                panic!("worker exited unexpectedly: {error}");
            }
        }));
    }

    test_latency(
        &store,
        Duration::milliseconds(500),
        5,
        50,
        Duration::seconds(20),
    )
    .await;

    scheduler_handle.abort();
    for worker_handle in worker_handles {
        worker_handle.abort();
    }
}

#[test]
async fn schedules_and_persistence() {
    let store = db_store().await;

    let scheduler = Scheduler::new(store.clone());
    let scheduler_handle = tokio::spawn(async move {
        if let Err(error) = scheduler.run().await {
            panic!("scheduler exited unexpectedly: {error}");
        }
    });

    let mut worker_handles = Vec::with_capacity(CONCURRENT_WORKERS);
    for _ in 0..CONCURRENT_WORKERS {
        let mut worker = Worker::new(store.clone());
        setup_worker(&mut worker);

        worker_handles.push(tokio::spawn(async move {
            if let Err(error) = worker.run().await {
                panic!("worker exited unexpectedly: {error}");
            }
        }));
    }

    test_schedules(&store, Duration::seconds(2)).await;

    scheduler_handle.abort();
    for worker_handle in worker_handles {
        worker_handle.abort();
    }

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

    let store = db_store().await;

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

    let mut worker = Worker::new(store.clone());
    setup_worker(&mut worker);

    let worker_handle = tokio::spawn(async move {
        if let Err(error) = worker.run().await {
            panic!("worker exited unexpectedly: {error}");
        }
    });

    for task in persistent_tasks {
        assert!(task.await.is_ok());
    }

    scheduler_handle.abort();
    worker_handle.abort();
}

#[test]
async fn schedule_cancellation() {
    let store = db_store().await;

    let scheduler = Scheduler::new(store.clone());
    let scheduler_handle = tokio::spawn(async move {
        if let Err(error) = scheduler.run().await {
            panic!("scheduler exited unexpectedly: {error}");
        }
    });

    let mut worker_handles = Vec::with_capacity(CONCURRENT_WORKERS);
    for _ in 0..CONCURRENT_WORKERS {
        let mut worker = Worker::new(store.clone());
        setup_worker(&mut worker);

        worker_handles.push(tokio::spawn(async move {
            if let Err(error) = worker.run().await {
                panic!("worker exited unexpectedly: {error}");
            }
        }));
    }

    test_schedule_cancellation(&store).await;

    scheduler_handle.abort();
    for worker_handle in worker_handles {
        worker_handle.abort();
    }
}
