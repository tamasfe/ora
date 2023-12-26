//! High-level and typed API for use in Rust applications and libraries.
#![warn(clippy::pedantic, missing_docs)]
#![allow(clippy::module_name_repetitions)]

use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use eyre::Context;
use ora_common::{
    task::{TaskDataFormat, TaskDefinition, TaskMetadata, WorkerSelector},
    timeout::TimeoutPolicy,
    UnixNanos,
};
use ora_worker::{registry::SupportedTask, RawHandler, TaskContext};
use serde::{de::DeserializeOwned, Serialize};

pub mod client;
pub mod defaults;

/// A strongly-typed serializable task type.
pub trait Task: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The output of the task.
    type Output: Serialize + DeserializeOwned + Send + Sync + 'static;

    /// The worker selector of the given type,
    /// defaults to the type's name without the module path
    /// and any generics.
    fn worker_selector() -> WorkerSelector {
        defaults::task_worker_selector::<Self>()
    }

    /// Input and output data format to be used for serialization.
    ///
    /// Defaults to [`TaskDataFormat::Json`].
    #[must_use]
    fn format() -> TaskDataFormat {
        defaults::task_data_format::<Self>()
    }

    /// Return the default timeout policy.
    ///
    /// Defaults to no timeout.
    #[must_use]
    fn timeout() -> TimeoutPolicy {
        defaults::task_timeout::<Self>()
    }

    /// Return additional metadata about the task.
    #[must_use]
    fn metadata() -> TaskMetadata {
        defaults::task_metadata::<Self>()
    }

    /// Create a task definition that contains the worker
    /// selector returned by [`Task::worker_selector`] and `self`
    /// serialized as the task data.
    ///
    /// # Panics
    ///
    /// Panics if [`Task::format`] is [`TaskDataFormat::Unknown`],
    /// or serialization fails.
    fn task(&self) -> TaskDefinition<Self>
    where
        Self: Serialize,
    {
        TaskDefinition {
            target: UnixNanos(0),
            worker_selector: Self::worker_selector(),
            data: match Self::format() {
                TaskDataFormat::Unknown => panic!("invalid data format"),
                TaskDataFormat::MessagePack => rmp_serde::to_vec_named(self).unwrap(),
                TaskDataFormat::Json => serde_json::to_vec(self).unwrap(),
            },
            data_format: Self::format(),
            labels: HashMap::default(),
            timeout: Self::timeout(),
            _task_type: PhantomData,
        }
    }
}

/// Additional trait that allows for setting
/// task defaults when deriving [`Task`].
pub trait DeriveTaskExtra: Task {
    /// The worker selector of the given type,
    /// defaults to the type's name without the module path
    /// and any generics.
    ///
    /// Default or inferred value are passed as argument.
    fn worker_selector(inferred: WorkerSelector) -> WorkerSelector {
        inferred
    }

    /// Input and output data format to be used for serialization.
    ///
    /// Default or inferred value are passed as argument.
    #[must_use]
    fn format(inferred: TaskDataFormat) -> TaskDataFormat {
        inferred
    }

    /// Return the default timeout policy.
    ///
    /// Default or inferred value are passed as argument.
    #[must_use]
    fn timeout(inferred: TimeoutPolicy) -> TimeoutPolicy {
        inferred
    }

    /// Return additional metadata about the task.
    ///
    /// Default or inferred value are passed as argument.
    #[must_use]
    fn metadata(inferred: TaskMetadata) -> TaskMetadata {
        inferred
    }
}

/// A strongly-typed worker that can
#[async_trait]
pub trait Handler<T>
where
    Self: Sized + Send + Sync + 'static,
    T: Task,
{
    /// Run the given task.
    async fn run(&self, ctx: TaskContext, task: T) -> eyre::Result<T::Output>;

    #[doc(hidden)]
    fn raw_handler(self) -> Arc<dyn RawHandler + Send + Sync> {
        self.raw_handler_with_selector(T::worker_selector())
    }

    #[doc(hidden)]
    fn raw_handler_with_selector(
        self,
        selector: WorkerSelector,
    ) -> Arc<dyn RawHandler + Send + Sync> {
        Arc::new(WorkerAdapter {
            selector,
            worker: self,
            _task: PhantomData,
        })
    }
}

#[async_trait]
impl<F, T, Fut> Handler<T> for F
where
    F: Fn(TaskContext, T) -> Fut + Send + Sync + 'static,
    T: Task,
    Fut: std::future::Future<Output = eyre::Result<T::Output>> + Send + 'static,
{
    async fn run(&self, ctx: TaskContext, task: T) -> eyre::Result<T::Output> {
        (self)(ctx, task).await
    }
}

/// A helper blanket trait for types that might implement [`Handler`]
/// for multiple [`Task`] types.
pub trait IntoHandler {
    /// Convert `self` into a [`RawHandler`] that can be registered
    /// in workers.
    fn handler<T>(self) -> Arc<dyn RawHandler + Send + Sync>
    where
        Self: Handler<T>,
        T: Task,
    {
        <Self as Handler<T>>::raw_handler(self)
    }

    /// Convert `self` into a [`RawHandler`] that can be registered
    /// in workers with the given selector.
    fn handler_with_selector<T>(self, selector: WorkerSelector) -> Arc<dyn RawHandler + Send + Sync>
    where
        Self: Handler<T>,
        T: Task,
    {
        <Self as Handler<T>>::raw_handler_with_selector(self, selector)
    }
}

impl<W> IntoHandler for W where W: Sized + Send + Sync + 'static {}

struct WorkerAdapter<W, T>
where
    T: Task,
    W: Handler<T>,
{
    worker: W,
    selector: WorkerSelector,
    _task: PhantomData<&'static T>,
}

#[async_trait]
impl<W, T> RawHandler for WorkerAdapter<W, T>
where
    T: Task,
    W: Handler<T> + Send + Sync + 'static,
{
    fn selector(&self) -> &WorkerSelector {
        &self.selector
    }

    fn output_format(&self) -> TaskDataFormat {
        T::format()
    }

    fn supported_task(&self) -> Option<SupportedTask> {
        Some(SupportedTask {
            worker_selector: self.selector.clone(),
            default_data_format: T::format(),
            default_timeout: T::timeout(),
            metadata: T::metadata(),
        })
    }

    async fn run(&self, context: TaskContext, task: TaskDefinition) -> eyre::Result<Vec<u8>> {
        let task: T = match task.data_format {
            TaskDataFormat::Unknown => eyre::bail!("failed to deserialize unknown data format"),
            TaskDataFormat::MessagePack => {
                rmp_serde::from_slice(&task.data).wrap_err("failed to deserialize MessagePack")?
            }
            TaskDataFormat::Json => {
                serde_json::from_slice(&task.data).wrap_err("failed to deserialize JSON")?
            }
        };

        let out = self.worker.run(context, task).await?;

        match T::format() {
            TaskDataFormat::Unknown => panic!("invalid task format"),
            TaskDataFormat::MessagePack => {
                Ok(rmp_serde::to_vec_named(&out).wrap_err("failed to deserialize output")?)
            }
            TaskDataFormat::Json => {
                Ok(serde_json::to_vec(&out).wrap_err("failed to deserialize output")?)
            }
        }
    }
}
