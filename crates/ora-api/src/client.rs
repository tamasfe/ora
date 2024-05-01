//! Types for interacting with an Ora store.

use std::{
    fmt::Debug,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use async_trait::async_trait;
use futures::{future::Shared, FutureExt};
use ora_client::{ClientOperations, RawTaskResult, ScheduleOperations, TaskOperations};
pub use ora_client::{
    LabelMatch, LabelValueMatch, ScheduleListOrder, Schedules, TaskListOrder, Tasks,
};
use ora_common::{
    schedule::ScheduleDefinition,
    task::{TaskDataFormat, TaskDefinition, TaskStatus},
};
use serde::de::DeserializeOwned;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::Task;

/// A client for interacting with the Ora framework.
///
/// This is usually implemented by a store or any interface
/// over a store.
#[async_trait]
pub trait Client: core::fmt::Debug + Send + Sync + 'static {
    /// Add a new task of the given type.
    async fn add_task<T>(&self, task: TaskDefinition<T>) -> eyre::Result<TaskHandle<T>>
    where
        T: Send + 'static;

    /// Add multiple tasks of a given type.
    ///
    /// This is more performant for some implementations
    /// than calling `add_tasks` repeatedly.
    async fn add_tasks<T, I>(&self, tasks: I) -> eyre::Result<Vec<TaskHandle<T>>>
    where
        T: Send + 'static,
        I: IntoIterator<Item = TaskDefinition<T>> + Send,
        I::IntoIter: ExactSizeIterator + Send;

    /// Retrieve a task by its ID.
    async fn task<T>(&self, task_id: Uuid) -> eyre::Result<TaskHandle<T>>
    where
        T: Send + 'static;

    /// Retrieve multiple tasks.
    ///
    /// The returned task handles are type-erased,
    /// individual tasks can be cast via [`TaskHandle::cast`]
    /// to specific types.
    async fn tasks(&self, options: &Tasks) -> eyre::Result<Vec<TaskHandle>>;

    /// Count current tasks.
    async fn task_count(&self, options: &Tasks) -> eyre::Result<u64>;

    /// Check whether any tasks exist that match the given options.
    async fn tasks_exist(&self, options: &Tasks) -> eyre::Result<bool>;

    /// Cancel all matching tasks and return handles to tasks
    /// that were cancelled.
    ///
    /// Cancellation is no-op on already finished tasks.
    ///
    /// The returned task handles are type-erased.
    async fn cancel_tasks(&self, options: &Tasks) -> eyre::Result<Vec<TaskHandle>>;

    /// Add a new schedule.
    async fn add_schedule(&self, schedule: ScheduleDefinition) -> eyre::Result<ScheduleHandle>;

    /// Add multiple schedules.
    ///
    /// This is more performant for some implementations
    /// than calling `add_schedule` repeatedly.
    async fn add_schedules<I>(&self, schedules: I) -> eyre::Result<Vec<ScheduleHandle>>
    where
        I: IntoIterator<Item = ScheduleDefinition> + Send,
        I::IntoIter: ExactSizeIterator + Send;

    /// Retrieve a schedule by its ID.
    async fn schedule(&self, schedule_id: Uuid) -> eyre::Result<ScheduleHandle>;

    /// Retrieve multiple schedules.
    async fn schedules(&self, options: &Schedules) -> eyre::Result<Vec<ScheduleHandle>>;

    /// Count current schedules.
    async fn schedule_count(&self, options: &Schedules) -> eyre::Result<u64>;

    /// Check whether any schedules exist that match the given options.
    async fn schedules_exist(&self, options: &Schedules) -> eyre::Result<bool>;

    /// Cancel all matching schedules and return handles to schedules
    /// that were cancelled.
    ///
    /// Cancellation is no-op on inactive schedules.
    async fn cancel_schedules(&self, options: &Schedules) -> eyre::Result<Vec<ScheduleHandle>>;
}

/// An object-safe version of [`Client`] with type-erased task information and
/// no generic arguments.
#[async_trait]
pub trait ClientObj {
    /// Add a new task of the given type.
    async fn add_task(&self, task: TaskDefinition) -> eyre::Result<TaskHandle>;

    /// Add multiple tasks of a given type.
    ///
    /// This is more performant for some implementations
    /// than calling `add_tasks` repeatedly.
    async fn add_tasks(
        &self,
        tasks: &mut (dyn ExactSizeIterator<Item = TaskDefinition> + Send),
    ) -> eyre::Result<Vec<TaskHandle>>;

    /// Retrieve a task by its ID.
    async fn task(&self, task_id: Uuid) -> eyre::Result<TaskHandle>;

    /// Retrieve multiple tasks.
    async fn tasks(&self, options: &Tasks) -> eyre::Result<Vec<TaskHandle>>;

    /// Cancel all matching tasks and return handles to tasks
    /// that were cancelled.
    ///
    /// Cancellation is no-op on already finished tasks.
    async fn cancel_tasks(&self, options: &Tasks) -> eyre::Result<Vec<TaskHandle>>;

    /// Count current tasks.
    async fn task_count(&self, options: &Tasks) -> eyre::Result<u64>;

    /// Check whether any tasks exist that match the given options.
    async fn tasks_exist(&self, options: &Tasks) -> eyre::Result<bool>;

    /// Add a new schedule.
    async fn add_schedule(&self, schedule: ScheduleDefinition) -> eyre::Result<ScheduleHandle>;

    /// Add multiple schedules.
    ///
    /// This is more performant for some implementations
    /// than calling `add_schedule` repeatedly.
    async fn add_schedules(
        &self,
        schedules: &mut (dyn ExactSizeIterator<Item = ScheduleDefinition> + Send),
    ) -> eyre::Result<Vec<ScheduleHandle>>;

    /// Retrieve a schedule by its ID.
    async fn schedule(&self, schedule_id: Uuid) -> eyre::Result<ScheduleHandle>;

    /// Retrieve multiple schedules.
    async fn schedules(&self, options: &Schedules) -> eyre::Result<Vec<ScheduleHandle>>;

    /// Count current schedules.
    async fn schedule_count(&self, options: &Schedules) -> eyre::Result<u64>;

    /// Check whether any schedules exist that match the given options.
    async fn schedules_exist(&self, options: &Schedules) -> eyre::Result<bool>;

    /// Cancel all matching schedules and return handles to schedules
    /// that were cancelled.
    ///
    /// Cancellation is no-op on inactive schedules.
    async fn cancel_schedules(&self, options: &Schedules) -> eyre::Result<Vec<ScheduleHandle>>;
}

#[async_trait]
impl<C> Client for C
where
    C: ClientOperations,
{
    async fn add_task<T>(&self, task: TaskDefinition<T>) -> eyre::Result<TaskHandle<T>>
    where
        T: Send + 'static,
    {
        let id = <Self as ClientOperations>::add_task(self, task.cast()).await?;
        <Self as Client>::task::<T>(self, id).await
    }

    async fn add_tasks<T, I>(&self, tasks: I) -> eyre::Result<Vec<TaskHandle<T>>>
    where
        I: IntoIterator<Item = TaskDefinition<T>> + Send,
        I::IntoIter: ExactSizeIterator + Send,
    {
        let ids = <Self as ClientOperations>::add_tasks(
            self,
            &mut tasks.into_iter().map(TaskDefinition::cast),
        )
        .await?;
        Ok(<Self as ClientOperations>::tasks_by_ids(self, ids)
            .await?
            .into_iter()
            .map(|ops| {
                let ops2 = ops.clone();
                TaskHandle {
                    id: ops.id(),
                    out: async move { Arc::new(ops2.wait_result().await) }
                        .boxed()
                        .shared(),
                    ops,
                    task_type: PhantomData,
                }
            })
            .collect())
    }

    async fn task<T>(&self, task_id: Uuid) -> eyre::Result<TaskHandle<T>> {
        Ok(TaskHandle::new_raw(
            <Self as ClientOperations>::task(self, task_id).await?,
        ))
    }

    async fn tasks(&self, options: &Tasks) -> eyre::Result<Vec<TaskHandle>> {
        Ok(<Self as ClientOperations>::tasks(self, options)
            .await?
            .into_iter()
            .map(|ops| {
                let ops2 = ops.clone();
                TaskHandle {
                    id: ops.id(),
                    out: async move { Arc::new(ops2.wait_result().await) }
                        .boxed()
                        .shared(),
                    ops,
                    task_type: PhantomData,
                }
            })
            .collect())
    }

    async fn task_count(&self, options: &Tasks) -> eyre::Result<u64> {
        <Self as ClientOperations>::task_count(self, options).await
    }

    async fn tasks_exist(&self, options: &Tasks) -> eyre::Result<bool> {
        <Self as ClientOperations>::tasks_exist(self, options).await
    }

    async fn cancel_tasks(&self, options: &Tasks) -> eyre::Result<Vec<TaskHandle>> {
        let options = options.clone().active(true).limit(u64::MAX);
        let ids = <Self as ClientOperations>::cancel_tasks(self, &options).await?;
        Ok(<Self as ClientOperations>::tasks_by_ids(self, ids)
            .await?
            .into_iter()
            .map(|ops| {
                let ops2 = ops.clone();
                TaskHandle {
                    id: ops.id(),
                    out: async move { Arc::new(ops2.wait_result().await) }
                        .boxed()
                        .shared(),
                    ops,
                    task_type: PhantomData,
                }
            })
            .collect())
    }

    async fn add_schedule(&self, schedule: ScheduleDefinition) -> eyre::Result<ScheduleHandle> {
        let id = <Self as ClientOperations>::add_schedule(self, schedule).await?;
        <Self as Client>::schedule(self, id).await
    }

    async fn add_schedules<I>(&self, schedules: I) -> eyre::Result<Vec<ScheduleHandle>>
    where
        I: IntoIterator<Item = ScheduleDefinition> + Send,
        I::IntoIter: ExactSizeIterator + Send,
    {
        let ids =
            <Self as ClientOperations>::add_schedules(self, &mut schedules.into_iter()).await?;
        Ok(<Self as ClientOperations>::schedules_by_ids(self, ids)
            .await?
            .into_iter()
            .map(|ops| ScheduleHandle { id: ops.id(), ops })
            .collect())
    }

    async fn schedule(&self, schedule_id: Uuid) -> eyre::Result<ScheduleHandle> {
        let ops = <Self as ClientOperations>::schedule(self, schedule_id).await?;
        Ok(ScheduleHandle {
            id: schedule_id,
            ops,
        })
    }

    async fn schedules(&self, options: &Schedules) -> eyre::Result<Vec<ScheduleHandle>> {
        Ok(<Self as ClientOperations>::schedules(self, options)
            .await?
            .into_iter()
            .map(|ops| ScheduleHandle { id: ops.id(), ops })
            .collect())
    }

    async fn schedule_count(&self, options: &Schedules) -> eyre::Result<u64> {
        <Self as ClientOperations>::schedule_count(self, options).await
    }

    async fn schedules_exist(&self, options: &Schedules) -> eyre::Result<bool> {
        <Self as ClientOperations>::schedules_exist(self, options).await
    }

    async fn cancel_schedules(&self, options: &Schedules) -> eyre::Result<Vec<ScheduleHandle>> {
        let options = options.clone().active(true).limit(u64::MAX);
        let ids = <Self as ClientOperations>::cancel_schedules(self, &options).await?;
        Ok(<Self as ClientOperations>::schedules_by_ids(self, ids)
            .await?
            .into_iter()
            .map(|ops| ScheduleHandle { id: ops.id(), ops })
            .collect())
    }
}

#[async_trait]
impl<C> ClientObj for C
where
    C: Client,
{
    async fn add_task(&self, task: TaskDefinition) -> eyre::Result<TaskHandle> {
        <Self as Client>::add_task(self, task).await
    }

    async fn add_tasks(
        &self,
        tasks: &mut (dyn ExactSizeIterator<Item = TaskDefinition> + Send),
    ) -> eyre::Result<Vec<TaskHandle>> {
        <Self as Client>::add_tasks(self, tasks).await
    }

    async fn task(&self, task_id: Uuid) -> eyre::Result<TaskHandle> {
        <Self as Client>::task(self, task_id).await
    }

    async fn tasks(&self, options: &Tasks) -> eyre::Result<Vec<TaskHandle>> {
        <Self as Client>::tasks(self, options).await
    }

    async fn cancel_tasks(&self, options: &Tasks) -> eyre::Result<Vec<TaskHandle>> {
        <Self as Client>::cancel_tasks(self, options).await
    }

    async fn task_count(&self, options: &Tasks) -> eyre::Result<u64> {
        <Self as Client>::task_count(self, options).await
    }

    async fn tasks_exist(&self, options: &Tasks) -> eyre::Result<bool> {
        <Self as Client>::tasks_exist(self, options).await
    }

    async fn add_schedule(&self, schedule: ScheduleDefinition) -> eyre::Result<ScheduleHandle> {
        <Self as Client>::add_schedule(self, schedule).await
    }

    async fn add_schedules(
        &self,
        schedules: &mut (dyn ExactSizeIterator<Item = ScheduleDefinition> + Send),
    ) -> eyre::Result<Vec<ScheduleHandle>> {
        <Self as Client>::add_schedules(self, schedules).await
    }

    async fn schedule(&self, schedule_id: Uuid) -> eyre::Result<ScheduleHandle> {
        <Self as Client>::schedule(self, schedule_id).await
    }

    async fn schedules(&self, options: &Schedules) -> eyre::Result<Vec<ScheduleHandle>> {
        <Self as Client>::schedules(self, options).await
    }

    async fn schedule_count(&self, options: &Schedules) -> eyre::Result<u64> {
        <Self as Client>::schedule_count(self, options).await
    }

    async fn schedules_exist(&self, options: &Schedules) -> eyre::Result<bool> {
        <Self as Client>::schedules_exist(self, options).await
    }

    async fn cancel_schedules(&self, options: &Schedules) -> eyre::Result<Vec<ScheduleHandle>> {
        <Self as Client>::cancel_schedules(self, options).await
    }
}

type SharedTaskOutputFut =
    Shared<Pin<Box<dyn Future<Output = Arc<eyre::Result<RawTaskResult>>> + Send>>>;

/// A handle to a task that can be used for further inspections and operations.
pub struct TaskHandle<T = ()> {
    id: Uuid,
    ops: Arc<dyn TaskOperations>,
    out: SharedTaskOutputFut,
    task_type: PhantomData<T>,
}

impl<T> Clone for TaskHandle<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            ops: self.ops.clone(),
            out: self.out.clone(),
            task_type: self.task_type,
        }
    }
}

impl<T> TaskHandle<T> {
    /// Create a new task handle over a raw implementation.
    pub fn new_raw(task_operations: Arc<dyn TaskOperations>) -> Self {
        let ops2 = task_operations.clone();
        TaskHandle {
            id: task_operations.id(),
            out: async move { Arc::new(ops2.wait_result().await) }
                .boxed()
                .shared(),
            ops: task_operations,
            task_type: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for TaskHandle<T>
where
    T: Debug + Unpin,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskHandle")
            .field("id", &self.id)
            .field("ops", &self.ops)
            .finish_non_exhaustive()
    }
}

#[allow(clippy::missing_errors_doc)]
impl<T> TaskHandle<T> {
    /// Return the task's ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }

    /// Return the task's current status.
    pub async fn status(&self) -> eyre::Result<TaskStatus> {
        self.ops.status().await
    }

    /// Return the task's target timestamp.
    pub async fn target(&self) -> eyre::Result<OffsetDateTime> {
        self.ops.target().await.map(Into::into)
    }

    /// Return the task's definition.
    pub async fn definition(&self) -> eyre::Result<TaskDefinition<T>> {
        self.ops.definition().await.map(TaskDefinition::cast)
    }

    /// Return the task's schedule, if any.
    pub async fn schedule(&self) -> eyre::Result<Option<ScheduleHandle>> {
        Ok(self
            .ops
            .schedule()
            .await?
            .map(|ops| ScheduleHandle { id: ops.id(), ops }))
    }

    /// Return when the task was added.
    pub async fn added_at(&self) -> eyre::Result<OffsetDateTime> {
        self.ops.added_at().await.map(Into::into)
    }

    /// Return when the task was ready.
    pub async fn ready_at(&self) -> eyre::Result<Option<OffsetDateTime>> {
        self.ops.ready_at().await.map(|v| v.map(Into::into))
    }
    /// Return when the task was started by a worker.
    pub async fn started_at(&self) -> eyre::Result<Option<OffsetDateTime>> {
        self.ops.started_at().await.map(|v| v.map(Into::into))
    }

    /// Return when the task was completed by a worker.
    pub async fn succeeded_at(&self) -> eyre::Result<Option<OffsetDateTime>> {
        self.ops.succeeded_at().await.map(|v| v.map(Into::into))
    }

    /// Return when the task failed.
    pub async fn failed_at(&self) -> eyre::Result<Option<OffsetDateTime>> {
        self.ops.failed_at().await.map(|v| v.map(Into::into))
    }

    /// Return when the task was cancelled.
    pub async fn cancelled_at(&self) -> eyre::Result<Option<OffsetDateTime>> {
        self.ops.cancelled_at().await.map(|v| v.map(Into::into))
    }

    /// Cancel a task, if the task has already finished
    /// running, this is a no-op.
    pub async fn cancel(&self) -> eyre::Result<()> {
        self.ops.cancel().await
    }

    /// Return the worker's ID that is associated with the task, if any.
    pub async fn worker_id(&self) -> eyre::Result<Option<Uuid>> {
        self.ops.worker_id().await
    }

    /// Cast the handle to a different task type or `()`
    /// to erase the type.
    #[must_use]
    pub fn cast<U>(self) -> TaskHandle<U> {
        TaskHandle {
            id: self.id,
            ops: self.ops,
            out: self.out,
            task_type: PhantomData,
        }
    }
}

impl<T> Future for TaskHandle<T>
where
    T: Task + Unpin,
{
    type Output = eyre::Result<T::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.out.poll_unpin(cx) {
            Poll::Ready(res) => match &*res {
                Ok(raw_out) => Poll::Ready(match &raw_out {
                    RawTaskResult::Success {
                        output_format,
                        output,
                    } => match output_format {
                        TaskDataFormat::Unknown => Err(eyre::eyre!("unknown format")),
                        TaskDataFormat::MessagePack => Ok(rmp_serde::from_slice(output)?),
                        TaskDataFormat::Json => Ok(serde_json::from_slice(output)?),
                    },
                    RawTaskResult::Failure { reason } => Err(eyre::eyre!("task failed: {reason}")),
                    RawTaskResult::Cancelled => Err(eyre::eyre!("task cancelled")),
                }),
                Err(error) => Poll::Ready(Err(eyre::eyre!("{error:?}"))),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Future for TaskHandle<()> {
    type Output = eyre::Result<TaskOutput>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.out.poll_unpin(cx) {
            Poll::Ready(res) => match &*res {
                Ok(raw_out) => Poll::Ready(match &raw_out {
                    RawTaskResult::Success {
                        output_format,
                        output,
                    } => Ok(TaskOutput {
                        output: output.clone(),
                        output_format: *output_format,
                    }),
                    RawTaskResult::Failure { reason } => Err(eyre::eyre!("task failed: {reason}")),
                    RawTaskResult::Cancelled => Err(eyre::eyre!("task cancelled")),
                }),
                Err(error) => Poll::Ready(Err(eyre::eyre!("{error:?}"))),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Output of a task.
#[derive(Debug)]
pub struct TaskOutput {
    output_format: TaskDataFormat,
    output: Vec<u8>,
}

impl TaskOutput {
    /// Deserialize the task's output.
    ///
    /// # Errors
    ///
    /// Deserialization errors are returned.
    pub fn get<T: DeserializeOwned>(&self) -> eyre::Result<T> {
        match self.output_format {
            TaskDataFormat::Unknown => eyre::bail!("unknown format"),
            TaskDataFormat::MessagePack => Ok(rmp_serde::from_slice(&self.output)?),
            TaskDataFormat::Json => Ok(serde_json::from_slice(&self.output)?),
        }
    }

    /// Expose the raw output bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.output
    }

    /// Convert into the raw output bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.output
    }

    /// Return the task's output format.
    #[must_use]
    pub fn format(&self) -> TaskDataFormat {
        self.output_format
    }
}

/// A handle to a schedule that allows inspection and operations.
#[derive(Debug, Clone)]
pub struct ScheduleHandle {
    id: Uuid,
    ops: Arc<dyn ScheduleOperations>,
}

#[allow(clippy::missing_errors_doc)]
impl ScheduleHandle {
    /// Create a new schedule handle over a raw implementation.
    pub fn new_raw(ops: Arc<dyn ScheduleOperations>) -> Self {
        ScheduleHandle { id: ops.id(), ops }
    }

    /// Return the schedule's ID.
    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Return the schedule's definition.
    pub async fn definition(&self) -> eyre::Result<ScheduleDefinition> {
        self.ops.definition().await
    }

    /// Return whether the schedule is currently active,
    /// meaning it has a currently running task or
    /// there can be more tasks spawned by it in the future.
    pub async fn is_active(&self) -> eyre::Result<bool> {
        self.ops.is_active().await
    }

    /// Return whether the schedule was cancelled.
    pub async fn cancelled_at(&self) -> eyre::Result<Option<OffsetDateTime>> {
        self.ops.cancelled_at().await.map(|v| v.map(Into::into))
    }

    /// Return the latest active task of the schedule.
    ///
    /// A task is considered active if it has not succeeded, has
    /// not failed and has not been cancelled.
    pub async fn active_task(&self) -> eyre::Result<Option<TaskHandle>> {
        let ops = self.ops.active_task().await?;
        Ok(ops.map(|ops| {
            let ops2 = ops.clone();
            TaskHandle {
                id: ops.id(),
                ops,
                out: async move { Arc::new(ops2.wait_result().await) }
                    .boxed()
                    .shared(),
                task_type: PhantomData,
            }
        }))
    }

    /// Cancel the schedule and all of its active tasks.
    pub async fn cancel(&self) -> eyre::Result<()> {
        self.ops.cancel().await
    }
}
