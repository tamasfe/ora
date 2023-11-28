//! Worker and worker implementations for Ora.

#![warn(clippy::pedantic, missing_docs)]
#![allow(clippy::module_name_repetitions, clippy::ignored_unit_patterns)]

use async_trait::async_trait;
use ora_common::task::{TaskDataFormat, TaskDefinition, WorkerSelector};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub mod store;
pub mod worker;

/// A context that is passed to each worker task execution.
#[derive(Debug, Clone)]
pub struct TaskContext {
    task_id: Uuid,
    cancellation: CancellationToken,
}

impl TaskContext {
    /// Return the task's ID.
    #[must_use]
    #[inline]
    pub const fn task_id(&self) -> Uuid {
        self.task_id
    }

    /// Return whether the task was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// Wait for task cancellation.
    pub async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }
}

/// A handler that works with raw input and output
/// without any task type information attached.
#[async_trait]
pub trait RawHandler {
    /// Return the selector that should be used to
    /// match tasks to this handler.
    fn selector(&self) -> &WorkerSelector;

    /// The data format of the task output.
    fn output_format(&self) -> TaskDataFormat;

    /// Execute a task.
    async fn run(&self, context: TaskContext, task: TaskDefinition) -> eyre::Result<Vec<u8>>;
}

// Not public API, do not use!
#[doc(hidden)]
pub mod _private {
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    use crate::TaskContext;

    #[must_use]
    pub fn new_context(task_id: Uuid, cancellation: CancellationToken) -> TaskContext {
        TaskContext {
            task_id,
            cancellation,
        }
    }
}
