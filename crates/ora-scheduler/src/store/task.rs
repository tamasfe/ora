//! Backend store types for managing tasks.
use async_trait::async_trait;
use futures::Stream;
use ora_common::{timeout::TimeoutPolicy, UnixNanos};
use uuid::Uuid;

/// A backend store for task management.
#[async_trait]
pub trait SchedulerTaskStore {
    /// An error type returned by operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// An event stream that can be used to watch for changes.
    type Events: Stream<Item = Result<SchedulerTaskStoreEvent, Self::Error>>;

    /// Subscribe for new events.
    async fn events(&self) -> Result<Self::Events, Self::Error>;

    /// Return all tasks that should be scheduled.
    async fn pending_tasks(&self) -> Result<Vec<PendingTask>, Self::Error>;

    /// Return active tasks that are not pending and are not
    /// yet finished.
    ///
    /// The scheduler needs to know about these tasks
    /// in order to schedule timeouts for all existing tasks,
    /// not just newly added ones.
    async fn active_tasks(&self) -> Result<Vec<ActiveTask>, Self::Error>;

    /// Update the task status as ready.
    async fn task_ready(&self, task_id: Uuid) -> Result<(), Self::Error>;

    /// The task timed out.
    ///
    /// This is always called for tasks that have a timeout policy set,
    /// it should be ignored if the task already finished before this
    /// function is called.
    async fn task_timed_out(&self, task_id: Uuid) -> Result<(), Self::Error>;
}

/// A task that was not yet marked as ready.
#[derive(Debug, Clone, Copy)]
pub struct PendingTask {
    /// The task's ID.
    pub id: Uuid,
    /// The task's target timestamp.
    pub target: UnixNanos,
    /// The task's timeout policy.
    pub timeout: TimeoutPolicy,
}

/// A task that is not finished and not pending anymore.
///
/// It is used to keep track of timeouts for tasks
/// that are already running when the scheduler starts.
#[derive(Debug, Clone, Copy)]
pub struct ActiveTask {
    /// The task's ID.
    pub id: Uuid,
    /// The task's target timestamp.
    pub target: UnixNanos,
    /// The task's timeout policy.
    pub timeout: TimeoutPolicy,
}

impl From<PendingTask> for ActiveTask {
    fn from(value: PendingTask) -> Self {
        Self {
            id: value.id,
            target: value.target,
            timeout: value.timeout,
        }
    }
}

/// A scheduler store event.
#[derive(Debug, Clone, Copy)]
pub enum SchedulerTaskStoreEvent {
    /// A new task was added.
    TaskAdded(PendingTask),
    /// A task was cancelled.
    TaskCancelled(Uuid),
}
