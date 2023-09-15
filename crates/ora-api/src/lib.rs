//! High-level and typed API for use in Rust applications and libraries.
#![warn(clippy::pedantic, missing_docs)]
#![allow(clippy::module_name_repetitions)]

use std::{any::type_name, collections::HashMap, marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use eyre::Context;
use ora_common::{
    task::{TaskDataFormat, TaskDefinition, WorkerSelector},
    timeout::TimeoutPolicy,
    UnixNanos,
};
use ora_worker::{RawHandler, TaskContext};
use serde::{de::DeserializeOwned, Serialize};

pub mod client;

/// A strongly-typed serializable task type.
pub trait Task: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The output of the task.
    type Output: Serialize + DeserializeOwned + Send + Sync + 'static;

    /// The worker selector of the given type,
    /// defaults to the type's name without the module path
    /// and any generics.
    fn worker_selector() -> WorkerSelector {
        let name = type_name::<Self>();
        let name = name.split_once('<').map_or(name, |n| n.0);
        let name = name.split("::").last().unwrap_or(name);

        WorkerSelector {
            kind: std::borrow::Cow::Borrowed(name),
        }
    }

    /// Input and output data format to be used for serialization.
    ///
    /// Defaults to [`TaskDataFormat::Json`].
    #[must_use]
    fn format() -> TaskDataFormat {
        TaskDataFormat::Json
    }

    /// Return the default timeout policy.
    ///
    /// Defaults to no timeout.
    #[must_use]
    fn timeout() -> TimeoutPolicy {
        TimeoutPolicy::Never
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
