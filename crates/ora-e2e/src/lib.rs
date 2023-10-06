//! Ora E2E test suite.

use async_trait::async_trait;
use ora::{
    client::Client, Handler, IntoHandler, NewTask, ScheduleDefinition, SchedulePolicy, Task,
    TaskContext, Worker,
};
use ora_client::{Schedules, Tasks};
use ora_common::task::TaskDefinition;
use tasks::{LatencyTestTask, ScheduleTestTask};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::tasks::ScheduleCancellationTestTask;

pub mod tasks;

struct TestHandler {}

#[async_trait]
impl Handler<LatencyTestTask> for TestHandler {
    async fn run(&self, ctx: TaskContext, task: LatencyTestTask) -> eyre::Result<Duration> {
        self.check_task_race(ctx.task_id());
        let now = OffsetDateTime::now_utc();
        Ok((now - task.target).abs())
    }
}

#[async_trait]
impl Handler<ScheduleTestTask> for TestHandler {
    async fn run(&self, ctx: TaskContext, _task: ScheduleTestTask) -> eyre::Result<()> {
        self.check_task_race(ctx.task_id());
        Ok(())
    }
}

#[async_trait]
impl Handler<ScheduleCancellationTestTask> for TestHandler {
    async fn run(&self, ctx: TaskContext, _task: ScheduleCancellationTestTask) -> eyre::Result<()> {
        self.check_task_race(ctx.task_id());
        Ok(())
    }
}

impl TestHandler {
    fn check_task_race(&self, task_id: Uuid) {
        static EXECUTED_TASK_IDS: once_cell::sync::Lazy<
            std::sync::Mutex<std::collections::HashSet<Uuid>>,
        > = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

        let mut executed_tasks_guard = EXECUTED_TASK_IDS.lock().unwrap();
        assert!(
            !executed_tasks_guard.contains(&task_id),
            "the task was executed multiple times"
        );

        executed_tasks_guard.insert(task_id);
    }
}

pub fn setup_worker<C>(worker: &mut Worker<C>) {
    worker.register_handler(TestHandler {}.handler::<LatencyTestTask>());
    worker.register_handler(TestHandler {}.handler::<ScheduleTestTask>());
}

pub async fn test_latency(
    store: &impl Client,
    interval: Duration,
    batch_count: usize,
    batch_task_count: usize,
    allowed_latency: Duration,
) {
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
}

pub async fn test_schedules(store: &impl Client, interval: Duration) {
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

pub async fn test_schedule_cancellation(store: &impl Client) {
    store
        .add_schedule(
            ScheduleDefinition::new(
                SchedulePolicy::repeat(Duration::milliseconds(100)),
                NewTask::repeat(ScheduleCancellationTestTask.task()),
            )
            .immediate(true),
        )
        .await
        .unwrap();

    // We give time for the scheduler to start a task.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    store
        .cancel_schedules(
            &Schedules::all().with_worker_selector(ScheduleCancellationTestTask::worker_selector()),
        )
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    assert_eq!(
        store
            .task_count(
                &Tasks::all()
                    .active(true)
                    .with_worker_selector(ScheduleCancellationTestTask::worker_selector())
            )
            .await
            .unwrap(),
        0
    );
}
