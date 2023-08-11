//! Ora in-memory store that does not persist task and schedule states.

#![warn(clippy::pedantic, missing_docs)]
#![allow(clippy::manual_let_else)]

use std::sync::Arc;

use ahash::HashSet;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::{stream::BoxStream, StreamExt};
use indexmap::IndexMap;
use ora_common::{
    schedule::ScheduleDefinition,
    task::{TaskDataFormat, TaskDefinition, TaskStatus, WorkerSelector},
    UnixNanos,
};
use ora_scheduler::store::{
    schedule::{ActiveSchedule, SchedulerScheduleStore, SchedulerScheduleStoreEvent},
    task::{PendingTask, SchedulerTaskStore, SchedulerTaskStoreEvent},
};
use ora_worker::store::{ReadyTask, WorkerPoolStore, WorkerPoolStoreEvent};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::sync::broadcast::{self, error::RecvError};
use uuid::Uuid;

mod client;

/// Store options.
#[derive(Debug)]
pub struct MemoryStoreOptions {
    /// Capacity used for internal broadcasting.
    pub channel_capacity: usize,
}

impl Default for MemoryStoreOptions {
    fn default() -> Self {
        Self {
            channel_capacity: 65536,
        }
    }
}

/// An in-memory store for Ora.
#[derive(Debug, Clone)]
#[must_use]
pub struct MemoryStore {
    inner: Arc<Inner>,
    scheduler_task_events: broadcast::Sender<SchedulerTaskStoreEvent>,
    scheduler_schedule_events: broadcast::Sender<SchedulerScheduleStoreEvent>,
    worker_pool_events: broadcast::Sender<WorkerPoolStoreEvent>,
}

impl MemoryStore {
    /// Creates a new [`MemoryStore`].
    pub fn new() -> Self {
        Self::new_with_options(MemoryStoreOptions::default())
    }

    /// Creates a new [`MemoryStore`] with the given options.
    #[allow(clippy::needless_pass_by_value)]
    pub fn new_with_options(options: MemoryStoreOptions) -> Self {
        let (scheduler_task_events, _) = broadcast::channel(options.channel_capacity);
        let (scheduler_schedule_events, _) = broadcast::channel(options.channel_capacity);
        let (worker_pool_events, _) = broadcast::channel(options.channel_capacity);

        Self {
            inner: Arc::new(Inner::default()),
            scheduler_task_events,
            scheduler_schedule_events,
            worker_pool_events,
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
struct Inner {
    tasks: Mutex<IndexMap<Uuid, TaskState>>,
    schedules: Mutex<IndexMap<Uuid, ScheduleState>>,
}

#[derive(Debug, Clone)]
struct TaskState {
    id: Uuid,
    status: TaskStatus,
    definition: TaskDefinition,
    output: Option<Vec<u8>>,
    output_format: TaskDataFormat,
    failure_reason: Option<String>,
    schedule_id: Option<Uuid>,
    added_at: UnixNanos,
    ready_at: Option<UnixNanos>,
    started_at: Option<UnixNanos>,
    succeeded_at: Option<UnixNanos>,
    failed_at: Option<UnixNanos>,
    cancelled_at: Option<UnixNanos>,
}

#[derive(Debug, Clone)]
struct ScheduleState {
    id: Uuid,
    definition: ScheduleDefinition,
    active: bool,
    active_task: Option<Uuid>,
    added_at: UnixNanos,
    cancelled_at: Option<UnixNanos>,
}

impl MemoryStore {
    fn cancel_task(&self, task_id: Uuid) -> Result<(), Error> {
        let mut tasks = self.inner.tasks.lock();
        let task = tasks
            .get_mut(&task_id)
            .ok_or(Error::TaskNotFound(task_id))?;

        if !task.status.is_finished() {
            task.cancelled_at = Some(UnixNanos::now());
            task.status = TaskStatus::Cancelled;

            let _ = self
                .scheduler_schedule_events
                .send(SchedulerScheduleStoreEvent::TaskFinished(task.id));
            let _ = self
                .scheduler_task_events
                .send(SchedulerTaskStoreEvent::TaskCancelled(task.id));
            let _ = self
                .worker_pool_events
                .send(WorkerPoolStoreEvent::TaskCancelled(task.id));
        }

        Ok(())
    }

    fn cancel_schedule(&self, schedule_id: Uuid) -> Result<(), Error> {
        let mut schedules = self.inner.schedules.lock();
        let schedule = schedules
            .get_mut(&schedule_id)
            .ok_or(Error::ScheduleNotFound(schedule_id))?;

        if let Some(active_task) = schedule.active_task {
            self.cancel_task(active_task)?;
        }

        if schedule.active {
            schedule.cancelled_at = Some(UnixNanos::now());

            let _ = self
                .scheduler_schedule_events
                .send(SchedulerScheduleStoreEvent::ScheduleCancelled(schedule.id));
        }
        schedule.active = false;
        schedule.active_task = None;

        Ok(())
    }

    fn task_finished(&self, task: &TaskState) -> Result<(), Error> {
        if let Some(schedule_id) = task.schedule_id {
            self.inner
                .schedules
                .lock()
                .get_mut(&schedule_id)
                .ok_or(Error::ScheduleNotFound(schedule_id))?
                .active_task = None;

            let _ = self
                .scheduler_schedule_events
                .send(SchedulerScheduleStoreEvent::TaskFinished(task.id));
        }

        Ok(())
    }
}

#[async_trait]
impl SchedulerTaskStore for MemoryStore {
    type Error = Error;
    type Events = BoxStream<'static, Result<SchedulerTaskStoreEvent, Self::Error>>;

    #[tracing::instrument(level = "debug", skip_all)]
    async fn events(&self) -> Result<Self::Events, Self::Error> {
        let mut events = self.scheduler_task_events.subscribe();

        Ok(try_stream!({
            loop {
                let res = events.recv().await;
                match res {
                    Ok(event) => {
                        yield event;
                    },
                    Err(error) => {
                        match error {
                            RecvError::Closed => {
                                return;
                            },
                            RecvError::Lagged(n) => {
                                panic!("in-memory channel lagged by {n}, increase the channel size to support larger volumes of events");
                            }
                        }
                    }
                }
            }
        })
        .boxed())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn pending_tasks(&self) -> Result<Vec<PendingTask>, Self::Error> {
        Ok(self
            .inner
            .tasks
            .lock()
            .values()
            .filter_map(|task| {
                if let TaskStatus::Pending = task.status {
                    Some(PendingTask {
                        id: task.id,
                        target: task.definition.target,
                        timeout: task.definition.timeout,
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn task_ready(&self, task_id: Uuid) -> Result<(), Self::Error> {
        if let Some(task) = self.inner.tasks.lock().get_mut(&task_id) {
            if !task.status.is_finished() {
                task.status = TaskStatus::Ready;
                task.ready_at = Some(UnixNanos::now());
                let _ = self
                    .worker_pool_events
                    .send(WorkerPoolStoreEvent::TaskReady(ReadyTask {
                        id: task_id,
                        definition: task.definition.clone(),
                    }));
            }
        } else {
            return Err(Error::TaskNotFound(task_id));
        }

        Ok(())
    }

    async fn task_timed_out(&self, task_id: Uuid) -> Result<(), Self::Error> {
        if let Some(task) = self.inner.tasks.lock().get_mut(&task_id) {
            if !task.status.is_finished() {
                task.status = TaskStatus::Failed;
                task.failure_reason = Some("task timeout reached".into());
                task.failed_at = Some(UnixNanos::now());
                let _ = self
                    .worker_pool_events
                    .send(WorkerPoolStoreEvent::TaskCancelled(task.id));
                self.task_finished(task)?;
            }
        } else {
            return Err(Error::TaskNotFound(task_id));
        }

        Ok(())
    }
}

#[async_trait]
impl SchedulerScheduleStore for MemoryStore {
    type Error = Error;

    type Events = BoxStream<'static, Result<SchedulerScheduleStoreEvent, Self::Error>>;

    async fn events(&self) -> Result<Self::Events, Self::Error> {
        let mut events = self.scheduler_schedule_events.subscribe();

        Ok(try_stream!({
            loop {
                let res = events.recv().await;
                match res {
                    Ok(event) => {
                        yield event;
                    },
                    Err(error) => {
                        match error {
                            RecvError::Closed => {
                                return;
                            },
                            RecvError::Lagged(n) => {
                                panic!("in-memory channel lagged by {n}, increase the channel size to support larger volumes of events");
                            }
                        }
                    }
                }
            }
        })
        .boxed())
    }

    async fn pending_schedules(&self) -> Result<Vec<ActiveSchedule>, Self::Error> {
        Ok(self
            .inner
            .schedules
            .lock()
            .values()
            .filter_map(|s| {
                if s.active_task.is_none() {
                    Some(ActiveSchedule {
                        id: s.id,
                        definition: s.definition.clone(),
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    async fn pending_schedule_of_task(
        &self,
        task_id: Uuid,
    ) -> Result<Option<ActiveSchedule>, Self::Error> {
        let schedule_id = self
            .inner
            .tasks
            .lock()
            .get(&task_id)
            .ok_or(Error::TaskNotFound(task_id))?
            .schedule_id;

        let schedule_id = match schedule_id {
            Some(id) => id,
            None => return Ok(None),
        };

        let schedules = self.inner.schedules.lock();
        let schedule = schedules
            .get(&schedule_id)
            .ok_or(Error::ScheduleNotFound(schedule_id))?;

        if schedule.active {
            Ok(Some(ActiveSchedule {
                id: schedule_id,
                definition: schedule.definition.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn add_task(&self, schedule_id: Uuid, task: TaskDefinition) -> Result<(), Self::Error> {
        let mut schedules = self.inner.schedules.lock();
        let schedule = schedules
            .get_mut(&schedule_id)
            .ok_or(Error::ScheduleNotFound(schedule_id))?;

        if !schedule.active {
            return Ok(());
        }

        assert!(schedule.active_task.is_none());

        let id = Uuid::new_v4();
        schedule.active_task = Some(id);
        drop(schedules);

        let target = task.target;
        let timeout = task.timeout;
        self.inner.tasks.lock().insert(
            id,
            TaskState {
                id,
                status: TaskStatus::Pending,
                definition: task,
                output: None,
                failure_reason: None,
                output_format: TaskDataFormat::default(),
                schedule_id: Some(schedule_id),
                added_at: UnixNanos::now(),
                ready_at: None,
                started_at: None,
                succeeded_at: None,
                failed_at: None,
                cancelled_at: None,
            },
        );

        let _ = self
            .scheduler_task_events
            .send(SchedulerTaskStoreEvent::TaskAdded(PendingTask {
                id,
                target,
                timeout,
            }));

        Ok(())
    }

    async fn task_target(&self, task_id: Uuid) -> Result<UnixNanos, Self::Error> {
        self.inner
            .tasks
            .lock()
            .get(&task_id)
            .ok_or_else(|| Error::TaskNotFound(task_id))
            .map(|t| t.definition.target)
    }
}

#[async_trait]
impl WorkerPoolStore for MemoryStore {
    type Error = Error;

    type Events = BoxStream<'static, Result<WorkerPoolStoreEvent, Self::Error>>;

    async fn events(&self, selectors: &[WorkerSelector]) -> Result<Self::Events, Self::Error> {
        let mut events = self.worker_pool_events.subscribe();
        let selectors = selectors.iter().cloned().collect::<HashSet<_>>();
        Ok(try_stream!({
            loop {
                let res = events.recv().await;
                match res {
                    Ok(event) => {
                        if let WorkerPoolStoreEvent::TaskReady(task) = &event {
                            if selectors.contains(&task.definition.worker_selector) {
                                yield event;
                            }
                        }
                    },
                    Err(error) => {
                        match error {
                            RecvError::Closed => {
                                return;
                            },
                            RecvError::Lagged(n) => {
                                panic!("in-memory channel lagged by {n}, increase the channel size to support larger volumes of events");
                            }
                        }
                    }
                }
            }
        })
        .boxed())
    }

    async fn ready_tasks(
        &self,
        selectors: &[WorkerSelector],
    ) -> Result<Vec<ReadyTask>, Self::Error> {
        Ok(self
            .inner
            .tasks
            .lock()
            .values()
            .filter_map(|task| {
                if selectors
                    .iter()
                    .any(|s| &task.definition.worker_selector == s)
                {
                    if let TaskStatus::Ready = task.status {
                        Some(ReadyTask {
                            id: task.id,
                            definition: task.definition.clone(),
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect())
    }

    async fn task_started(&self, task_id: Uuid) -> Result<(), Self::Error> {
        if let Some(task) = self.inner.tasks.lock().get_mut(&task_id) {
            task.status = TaskStatus::Started;
            task.started_at = Some(UnixNanos::now());
        } else {
            return Err(Error::TaskNotFound(task_id));
        }
        Ok(())
    }

    async fn task_succeeded(
        &self,
        task_id: Uuid,
        output: Vec<u8>,
        output_format: TaskDataFormat,
    ) -> Result<(), Self::Error> {
        if let Some(task) = self.inner.tasks.lock().get_mut(&task_id) {
            task.output = Some(output);
            task.status = TaskStatus::Succeeded;
            task.output_format = output_format;
            task.succeeded_at = Some(UnixNanos::now());
            self.task_finished(task)?;
        } else {
            return Err(Error::TaskNotFound(task_id));
        }

        Ok(())
    }

    async fn task_failed(&self, task_id: Uuid, reason: String) -> Result<(), Self::Error> {
        if let Some(task) = self.inner.tasks.lock().get_mut(&task_id) {
            task.failure_reason = Some(reason);
            task.status = TaskStatus::Failed;
            task.failed_at = Some(UnixNanos::now());
            self.task_finished(task)?;
        } else {
            return Err(Error::TaskNotFound(task_id));
        }
        Ok(())
    }
}

/// Memory store error type.
#[derive(Debug, Error)]
pub enum Error {
    /// The requested task was not found.
    #[error("task was not found with id `{0}`")]
    TaskNotFound(Uuid),
    /// The requested schedule was not found.
    #[error("schedule was not found with id `{0}`")]
    ScheduleNotFound(Uuid),
}
