//! Backend store implementations required by worker pools.

use async_trait::async_trait;
use futures::Stream;
use ora_common::task::{TaskDataFormat, TaskDefinition, WorkerSelector};
use uuid::Uuid;

/// A store
#[async_trait]
pub trait WorkerPoolStore: Send + Sync + Clone {
    /// An error type returned by operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// An event stream that can be used to watch for changes.
    type Events: Stream<Item = Result<WorkerPoolStoreEvent, Self::Error>>;

    /// Subscribe for new events with the given worker selectors.
    async fn events(&self, selectors: &[WorkerSelector]) -> Result<Self::Events, Self::Error>;

    /// Return all tasks that should be executed with any of the given worker selectors.
    async fn ready_tasks(
        &self,
        selectors: &[WorkerSelector],
    ) -> Result<Vec<ReadyTask>, Self::Error>;

    /// Update the task status as started.
    async fn task_started(&self, task_id: Uuid) -> Result<(), Self::Error>;

    /// Update the task status as successful with the given output.
    async fn task_succeeded(
        &self,
        task_id: Uuid,
        output: Vec<u8>,
        output_format: TaskDataFormat,
    ) -> Result<(), Self::Error>;

    /// Update the task status as failed with the given reason.
    async fn task_failed(&self, task_id: Uuid, reason: String) -> Result<(), Self::Error>;
}

/// A task that is ready to be run by a worker.
#[derive(Debug, Clone)]
pub struct ReadyTask {
    /// The task's ID.
    pub id: Uuid,
    /// The task's definition.
    pub definition: TaskDefinition,
}

/// An event returned by the store.
#[derive(Debug, Clone)]
pub enum WorkerPoolStoreEvent {
    /// A task is ready to be run.
    TaskReady(ReadyTask),
    /// A task was cancelled.
    TaskCancelled(Uuid),
}
