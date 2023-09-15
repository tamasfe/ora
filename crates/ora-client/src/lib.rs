//! Common interface that should be implemented by clients that interact
//! with the underlying store and are capable of managing tasks and schedules.
//!
//! The interfaces follow an object-oriented structure with type-erased trait
//! objects that allow operations on specific items. While in general less efficient,
//! this structure allows for an ergonomic API and it's easy to implement graph-like
//! APIs over it (like GraphQL).
#![warn(clippy::pedantic)]

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use ora_common::{
    schedule::ScheduleDefinition,
    task::{TaskDataFormat, TaskDefinition, TaskStatus, WorkerSelector},
    UnixNanos,
};
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

#[async_trait]
pub trait ClientOperations: core::fmt::Debug + Send + Sync + 'static {
    async fn add_task(&self, task: TaskDefinition) -> eyre::Result<Uuid>;
    async fn task(&self, task_id: Uuid) -> eyre::Result<Arc<dyn TaskOperations>>;
    async fn tasks(&self, options: &Tasks) -> eyre::Result<Vec<Arc<dyn TaskOperations>>>;
    async fn task_count(&self, options: &Tasks) -> eyre::Result<u64>;
    async fn task_labels(&self) -> eyre::Result<Vec<String>>;
    async fn task_kinds(&self) -> eyre::Result<Vec<String>>;

    async fn add_schedule(&self, schedule: ScheduleDefinition) -> eyre::Result<Uuid>;
    async fn schedule(&self, schedule_id: Uuid) -> eyre::Result<Arc<dyn ScheduleOperations>>;
    async fn schedules(
        &self,
        options: &Schedules,
    ) -> eyre::Result<Vec<Arc<dyn ScheduleOperations>>>;
    async fn schedule_count(&self, options: &Schedules) -> eyre::Result<u64>;
    async fn schedule_labels(&self) -> eyre::Result<Vec<String>>;

    async fn add_tasks(
        &self,
        tasks: &mut (dyn ExactSizeIterator<Item = TaskDefinition> + Send),
    ) -> eyre::Result<Vec<Uuid>> {
        let mut ids = Vec::with_capacity(tasks.len());

        for task in tasks {
            ids.push(self.add_task(task).await?);
        }

        Ok(ids)
    }

    async fn cancel_tasks(&self, options: &Tasks) -> eyre::Result<Vec<Uuid>> {
        let tasks = self.tasks(options).await?;
        let mut task_ids = Vec::new();

        for task in tasks {
            if !task.status().await?.is_finished() {
                task.cancel().await?;
                task_ids.push(task.id());
            }
        }

        Ok(task_ids)
    }

    async fn tasks_by_ids(
        &self,
        task_ids: Vec<Uuid>,
    ) -> eyre::Result<Vec<Arc<dyn TaskOperations>>> {
        let mut tasks = Vec::with_capacity(task_ids.len());
        for task_id in task_ids {
            tasks.push(self.task(task_id).await?);
        }
        Ok(tasks)
    }

    async fn add_schedules(
        &self,
        schedules: &mut (dyn ExactSizeIterator<Item = ScheduleDefinition> + Send),
    ) -> eyre::Result<Vec<Uuid>> {
        let mut ids = Vec::new();

        for schedule in schedules {
            ids.push(self.add_schedule(schedule).await?);
        }

        Ok(ids)
    }

    async fn schedules_by_ids(
        &self,
        schedule_ids: Vec<Uuid>,
    ) -> eyre::Result<Vec<Arc<dyn ScheduleOperations>>> {
        let mut schedules = Vec::with_capacity(schedule_ids.len());
        for schedule_id in schedule_ids {
            schedules.push(self.schedule(schedule_id).await?);
        }
        Ok(schedules)
    }

    async fn cancel_schedules(&self, options: &Schedules) -> eyre::Result<Vec<Uuid>> {
        let schedules = self.schedules(options).await?;
        let mut schedule_ids = Vec::new();

        for schedule in schedules {
            if schedule.is_active().await? {
                schedule.cancel().await?;
                schedule_ids.push(schedule.id());
            }
        }

        Ok(schedule_ids)
    }
}

#[async_trait]
pub trait TaskOperations: core::fmt::Debug + Send + Sync + 'static {
    fn id(&self) -> Uuid;
    async fn status(&self) -> eyre::Result<TaskStatus>;
    async fn target(&self) -> eyre::Result<UnixNanos>;
    async fn definition(&self) -> eyre::Result<TaskDefinition>;
    async fn result(&self) -> eyre::Result<Option<RawTaskResult>>;
    async fn wait_result(&self) -> eyre::Result<RawTaskResult>;
    async fn schedule(&self) -> eyre::Result<Option<Arc<dyn ScheduleOperations>>>;
    async fn added_at(&self) -> eyre::Result<UnixNanos>;
    async fn ready_at(&self) -> eyre::Result<Option<UnixNanos>>;
    async fn started_at(&self) -> eyre::Result<Option<UnixNanos>>;
    async fn succeeded_at(&self) -> eyre::Result<Option<UnixNanos>>;
    async fn failed_at(&self) -> eyre::Result<Option<UnixNanos>>;
    async fn cancelled_at(&self) -> eyre::Result<Option<UnixNanos>>;
    async fn cancel(&self) -> eyre::Result<()>;
    async fn worker_id(&self) -> eyre::Result<Option<Uuid>>;
}

#[async_trait]
impl TaskOperations for Arc<dyn TaskOperations> {
    fn id(&self) -> Uuid {
        (**self).id()
    }

    async fn status(&self) -> eyre::Result<TaskStatus> {
        (**self).status().await
    }

    async fn target(&self) -> eyre::Result<UnixNanos> {
        (**self).target().await
    }

    async fn definition(&self) -> eyre::Result<TaskDefinition> {
        (**self).definition().await
    }

    async fn result(&self) -> eyre::Result<Option<RawTaskResult>> {
        (**self).result().await
    }

    async fn wait_result(&self) -> eyre::Result<RawTaskResult> {
        (**self).wait_result().await
    }

    async fn schedule(&self) -> eyre::Result<Option<Arc<dyn ScheduleOperations>>> {
        (**self).schedule().await
    }

    async fn added_at(&self) -> eyre::Result<UnixNanos> {
        (**self).added_at().await
    }

    async fn ready_at(&self) -> eyre::Result<Option<UnixNanos>> {
        (**self).ready_at().await
    }

    async fn started_at(&self) -> eyre::Result<Option<UnixNanos>> {
        (**self).started_at().await
    }

    async fn succeeded_at(&self) -> eyre::Result<Option<UnixNanos>> {
        (**self).succeeded_at().await
    }

    async fn failed_at(&self) -> eyre::Result<Option<UnixNanos>> {
        (**self).failed_at().await
    }

    async fn cancelled_at(&self) -> eyre::Result<Option<UnixNanos>> {
        (**self).cancelled_at().await
    }

    async fn cancel(&self) -> eyre::Result<()> {
        (**self).cancel().await
    }

    async fn worker_id(&self) -> eyre::Result<Option<Uuid>> {
        (**self).worker_id().await
    }
}

#[async_trait]
pub trait ScheduleOperations: core::fmt::Debug + Send + Sync + 'static {
    fn id(&self) -> Uuid;
    async fn definition(&self) -> eyre::Result<ScheduleDefinition>;
    async fn is_active(&self) -> eyre::Result<bool>;
    async fn added_at(&self) -> eyre::Result<UnixNanos>;
    async fn cancelled_at(&self) -> eyre::Result<Option<UnixNanos>>;
    async fn active_task(&self) -> eyre::Result<Option<Arc<dyn TaskOperations>>>;
    async fn cancel(&self) -> eyre::Result<()>;
}

#[async_trait]
impl ScheduleOperations for Arc<dyn ScheduleOperations> {
    fn id(&self) -> Uuid {
        (**self).id()
    }

    async fn definition(&self) -> eyre::Result<ScheduleDefinition> {
        (**self).definition().await
    }

    async fn is_active(&self) -> eyre::Result<bool> {
        (**self).is_active().await
    }

    async fn added_at(&self) -> eyre::Result<UnixNanos> {
        (**self).added_at().await
    }

    async fn cancelled_at(&self) -> eyre::Result<Option<UnixNanos>> {
        (**self).cancelled_at().await
    }

    async fn active_task(&self) -> eyre::Result<Option<Arc<dyn TaskOperations>>> {
        (**self).active_task().await
    }

    async fn cancel(&self) -> eyre::Result<()> {
        (**self).cancel().await
    }
}

/// A task result without additional context.
#[derive(Debug, Clone)]
pub enum RawTaskResult {
    /// The task succeeded.
    Success {
        /// The output format of the task,
        /// use the input data format if not known.
        output_format: TaskDataFormat,
        /// The output bytes.
        output: Vec<u8>,
    },
    /// The task failed.
    Failure {
        /// Reason of failure.
        reason: String,
    },
    /// The task was cancelled.
    Cancelled,
}

impl RawTaskResult {
    /// Returns `true` if the raw task result is [`Success`].
    ///
    /// [`Success`]: RawTaskResult::Success
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns `true` if the raw task result is [`Failure`].
    ///
    /// [`Failure`]: RawTaskResult::Failure
    #[must_use]
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }

    /// Returns `true` if the raw task result is [`Cancelled`].
    ///
    /// [`Cancelled`]: RawTaskResult::Cancelled
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// Search options for retrieving a list of tasks.
#[derive(Debug, Clone)]
#[must_use]
pub struct Schedules {
    /// Only include schedules that are either active or inactive.
    pub active: Option<bool>,
    /// Only include schedules added after the given timestamp (inclusive).
    pub added_after: Option<OffsetDateTime>,
    /// Only include schedules added before the given timestamp (exclusive).
    pub added_before: Option<OffsetDateTime>,
    /// Only include schedules cancelled after the given timestamp (inclusive).
    pub cancelled_after: Option<OffsetDateTime>,
    /// Only include schedules cancelled before the given timestamp (exclusive).
    pub cancelled_before: Option<OffsetDateTime>,
    /// A search text, how it is used is up to the store.
    pub search: Option<String>,
    /// Include only matching labels.
    pub include_labels: Option<Vec<LabelMatch>>,
    /// Only return schedules that are known to spawn tasks
    /// with the given selector kind.
    pub kind: Option<String>,
    /// The way tasks are ordered before applying
    /// offset and limit.
    pub order: ScheduleListOrder,
    /// The offset.
    pub offset: u64,
    /// The maximum amount of schedules to fetch.
    pub limit: u64,
}

impl Schedules {
    /// Include all schedules without applying any filters nor limits.
    ///
    /// **caution**: It does include ALL schedules without a limit.
    pub fn all() -> Self {
        Self {
            limit: u64::MAX,
            ..Self::default()
        }
    }

    /// Incldue active or inactive schedules only.
    pub fn active(mut self, active: bool) -> Self {
        self.active = Some(active);
        self
    }

    /// Select schedules that contain the given label with any value.
    pub fn with_label(mut self, label: &str) -> Self {
        let mut labels = self.include_labels.take().unwrap_or_default();
        labels.push(LabelMatch {
            label: label.to_string(),
            value: LabelValueMatch::AnyValue,
        });
        self.include_labels = Some(labels);

        self
    }

    /// Select schedules that contain the given label and a specific value.
    ///
    /// # Panics
    ///
    /// Panics if the given value is not JSON-serializable.
    pub fn with_label_value(mut self, label: &str, value: impl Serialize) -> Self {
        let mut labels = self.include_labels.take().unwrap_or_default();
        labels.push(LabelMatch {
            label: label.to_string(),
            value: LabelValueMatch::Value(serde_json::to_value(value).unwrap()),
        });
        self.include_labels = Some(labels);

        self
    }

    /// Only include schedules with a known matching worker selector.
    pub fn with_worker_selector(mut self, selector: WorkerSelector) -> Self {
        self.kind = Some(selector.kind.into());
        self
    }
}

impl Default for Schedules {
    fn default() -> Self {
        Self {
            active: None,
            include_labels: None,
            kind: None,
            order: ScheduleListOrder::default(),
            search: None,
            offset: 0,
            limit: u64::MAX,
            added_after: None,
            added_before: None,
            cancelled_after: None,
            cancelled_before: None,
        }
    }
}

/// The ordering of returned schedules.
#[derive(Debug, Clone, Copy, Default)]
pub enum ScheduleListOrder {
    AddedAsc,
    #[default]
    AddedDesc,
}

/// Allow matching on labels.
#[derive(Debug, Clone)]
pub struct LabelMatch {
    /// The name of the label.
    pub label: String,
    /// The value of the label.
    pub value: LabelValueMatch,
}

/// Match label values.
#[derive(Debug, Clone, Default)]
pub enum LabelValueMatch {
    /// Match any label value.
    #[default]
    AnyValue,
    /// Match the exact value.
    Value(Value),
}

impl LabelValueMatch {
    #[must_use]
    pub fn as_value(&self) -> Option<&Value> {
        if let Self::Value(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

/// Search options for retrieving a list of tasks.
#[derive(Debug, Clone)]
#[must_use]
pub struct Tasks {
    /// Only return tasks with the given status.
    pub include_status: Option<HashSet<TaskStatus>>,
    /// Include only matching labels.
    pub include_labels: Option<Vec<LabelMatch>>,
    /// Only return tasks for a given schedule.
    pub schedule_id: Option<Uuid>,
    /// A search text, how it is used is up to the store.
    pub search: Option<String>,

    /// Only include tasks added after the given timestamp (inclusive).
    pub added_after: Option<OffsetDateTime>,
    /// Only include tasks added before the given timestamp (exclusive).
    pub added_before: Option<OffsetDateTime>,
    /// Only include tasks finished after the given timestamp (inclusive).
    pub finished_after: Option<OffsetDateTime>,
    /// Only include tasks finished before the given timestamp (exclusive).
    pub finished_before: Option<OffsetDateTime>,
    /// Only include tasks with a target after the given timestamp (inclusive).
    pub target_after: Option<OffsetDateTime>,
    /// Only include tasks with a target before the given timestamp (exclusive).
    pub target_before: Option<OffsetDateTime>,

    /// Only return tasks with the provided worker selector kind.
    pub kind: Option<String>,
    /// The way tasks are ordered before applying
    /// offset and limit.
    pub order: TaskListOrder,
    /// The task offset.
    pub offset: u64,
    /// The maximum amount of tasks to fetch.
    pub limit: u64,
}

impl Tasks {
    /// Include all tasks without applying any filters nor limits.
    ///
    /// **caution**: It does include ALL tasks without a limit.
    pub fn all() -> Self {
        Self {
            limit: u64::MAX,
            ..Self::default()
        }
    }

    /// Set the maximum amount of tasks that is retrieved.
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }

    /// Only include active or inactive tasks.
    pub fn active(mut self, active: bool) -> Self {
        if active {
            self.include_status = Some(
                [TaskStatus::Pending, TaskStatus::Ready, TaskStatus::Started]
                    .into_iter()
                    .collect(),
            );
        } else {
            self.include_status = Some(
                [
                    TaskStatus::Succeeded,
                    TaskStatus::Failed,
                    TaskStatus::Cancelled,
                ]
                .into_iter()
                .collect(),
            );
        }
        self
    }

    /// Only include tasks with a matching worker selector.
    pub fn with_worker_selector(mut self, selector: WorkerSelector) -> Self {
        self.kind = Some(selector.kind.into());
        self
    }

    /// Select tasks that contain the given label with any value.
    pub fn with_label(mut self, label: &str) -> Self {
        let mut labels = self.include_labels.take().unwrap_or_default();
        labels.push(LabelMatch {
            label: label.to_string(),
            value: LabelValueMatch::AnyValue,
        });
        self.include_labels = Some(labels);

        self
    }

    /// Select tasks that contain the given label and a specific value.
    ///
    /// # Panics
    ///
    /// Panics if the given value is not JSON-serializable.
    pub fn with_label_value(mut self, label: &str, value: impl Serialize) -> Self {
        let mut labels = self.include_labels.take().unwrap_or_default();
        labels.push(LabelMatch {
            label: label.to_string(),
            value: LabelValueMatch::Value(serde_json::to_value(value).unwrap()),
        });
        self.include_labels = Some(labels);

        self
    }
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            include_status: None,
            include_labels: None,
            schedule_id: None,
            search: None,
            kind: None,
            order: TaskListOrder::default(),
            offset: 0,
            limit: u64::MAX,
            added_after: None,
            added_before: None,
            finished_after: None,
            finished_before: None,
            target_after: None,
            target_before: None,
        }
    }
}

/// The ordering of returned tasks.
#[derive(Debug, Clone, Copy, Default)]
pub enum TaskListOrder {
    AddedAsc,
    #[default]
    AddedDesc,
    TargetAsc,
    TargetDesc,
}
