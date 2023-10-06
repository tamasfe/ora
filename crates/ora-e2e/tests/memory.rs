use ora::{Client, MemoryStore, MemoryStoreOptions, Scheduler, Task, Worker};
use ora_client::Tasks;
use ora_e2e::{
    setup_worker, tasks::LatencyTestTask, test_latency, test_schedule_cancellation, test_schedules,
};
use time::{Duration, OffsetDateTime};
use tokio::test;
use uuid::Uuid;

const CONCURRENT_WORKERS: usize = 50;

async fn memory_store() -> MemoryStore {
    MemoryStore::new_with_options(MemoryStoreOptions {
        channel_capacity: 10_000,
    })
}

#[test]
async fn latency() {
    let store = memory_store().await;

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
        10,
        Duration::milliseconds(1),
    )
    .await;

    test_latency(
        &store,
        Duration::milliseconds(500),
        5,
        1000,
        Duration::milliseconds(50),
    )
    .await;

    scheduler_handle.abort();

    for worker_handle in worker_handles {
        worker_handle.abort();
    }
}

#[test]
async fn schedules_and_restarts() {
    let store = memory_store().await;

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
    let store = memory_store().await;

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
