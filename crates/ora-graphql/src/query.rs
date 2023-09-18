use std::{collections::HashSet, sync::Arc};

use async_graphql::{Enum, InputObject, Object};
use ora_client::{
    ClientOperations, LabelMatch, LabelValueMatch, ScheduleListOrder, ScheduleOperations,
    Schedules, TaskListOrder, TaskOperations, Tasks,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::common::{GqlScheduleDefinition, GqlTaskDefinition, GqlTaskResult, GqlTaskStatus};

pub struct Query {
    pub(crate) client: Arc<dyn ClientOperations>,
}

#[Object]
impl Query {
    async fn task_count(&self, options: Option<GqlTaskListOptions>) -> async_graphql::Result<u64> {
        let options = options.map(Tasks::from).unwrap_or_default();
        self.client
            .task_count(&options)
            .await
            .map_err(async_graphql::Error::new_with_source)
    }

    async fn task(&self, id: Uuid) -> async_graphql::Result<Task> {
        let ops = self
            .client
            .task(id)
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(Task {
            client: self.client.clone(),
            ops,
        })
    }

    async fn tasks(&self, options: Option<GqlTaskListOptions>) -> async_graphql::Result<Vec<Task>> {
        let options = options.map(Tasks::from).unwrap_or_default();

        Ok(self
            .client
            .tasks(&options)
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into_iter()
            .map(|ops| Task {
                client: self.client.clone(),
                ops,
            })
            .collect())
    }

    async fn task_labels(&self) -> async_graphql::Result<Vec<String>> {
        self.client
            .task_labels()
            .await
            .map_err(async_graphql::Error::new_with_source)
    }

    async fn task_kinds(&self) -> async_graphql::Result<Vec<String>> {
        self.client
            .task_kinds()
            .await
            .map_err(async_graphql::Error::new_with_source)
    }

    async fn schedule(&self, id: Uuid) -> async_graphql::Result<Schedule> {
        let ops = self
            .client
            .schedule(id)
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(Schedule {
            client: self.client.clone(),
            ops,
        })
    }

    async fn schedules(
        &self,
        options: Option<GqlScheduleListOptions>,
    ) -> async_graphql::Result<Vec<Schedule>> {
        let options: Schedules = options.map(Schedules::from).unwrap_or_default();
        Ok(self
            .client
            .schedules(&options)
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into_iter()
            .map(|ops| Schedule {
                client: self.client.clone(),
                ops,
            })
            .collect())
    }

    async fn schedule_count(
        &self,
        options: Option<GqlScheduleListOptions>,
    ) -> async_graphql::Result<u64> {
        let options = options.map(Schedules::from).unwrap_or_default();
        self.client
            .schedule_count(&options)
            .await
            .map_err(async_graphql::Error::new_with_source)
    }
}

#[derive(Debug)]
pub(crate) struct Task {
    pub(crate) client: Arc<dyn ClientOperations>,
    pub(crate) ops: Arc<dyn TaskOperations>,
}

#[Object]
impl Task {
    async fn id(&self) -> Uuid {
        self.ops.id()
    }
    async fn status(&self) -> async_graphql::Result<GqlTaskStatus> {
        Ok(self
            .ops
            .status()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into())
    }
    async fn definition(&self) -> async_graphql::Result<GqlTaskDefinition> {
        Ok(self
            .ops
            .definition()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into())
    }
    async fn target(&self) -> async_graphql::Result<OffsetDateTime> {
        Ok(self
            .ops
            .target()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into())
    }
    async fn schedule(&self) -> async_graphql::Result<Option<Schedule>> {
        Ok(self
            .ops
            .schedule()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .map(|ops| Schedule {
                client: self.client.clone(),
                ops,
            }))
    }
    async fn added_at(&self) -> async_graphql::Result<OffsetDateTime> {
        Ok(self
            .ops
            .added_at()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into())
    }
    async fn ready_at(&self) -> async_graphql::Result<Option<OffsetDateTime>> {
        Ok(self
            .ops
            .ready_at()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .map(Into::into))
    }
    async fn started_at(&self) -> async_graphql::Result<Option<OffsetDateTime>> {
        Ok(self
            .ops
            .started_at()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .map(Into::into))
    }
    async fn succeeded_at(&self) -> async_graphql::Result<Option<OffsetDateTime>> {
        Ok(self
            .ops
            .succeeded_at()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .map(Into::into))
    }
    async fn failed_at(&self) -> async_graphql::Result<Option<OffsetDateTime>> {
        Ok(self
            .ops
            .failed_at()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .map(Into::into))
    }
    async fn cancelled_at(&self) -> async_graphql::Result<Option<OffsetDateTime>> {
        Ok(self
            .ops
            .cancelled_at()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .map(Into::into))
    }

    async fn result(&self) -> async_graphql::Result<Option<GqlTaskResult>> {
        Ok(self
            .ops
            .result()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .map(Into::into))
    }

    async fn worker_id(&self) -> async_graphql::Result<Option<Uuid>> {
        self.ops
            .worker_id()
            .await
            .map_err(async_graphql::Error::new_with_source)
    }
}

#[derive(Debug)]
pub(crate) struct Schedule {
    pub(crate) client: Arc<dyn ClientOperations>,
    pub(crate) ops: Arc<dyn ScheduleOperations>,
}

#[Object]
impl Schedule {
    async fn id(&self) -> Uuid {
        self.ops.id()
    }

    async fn active(&self) -> async_graphql::Result<bool> {
        self.ops
            .is_active()
            .await
            .map_err(async_graphql::Error::new_with_source)
    }

    async fn added_at(&self) -> async_graphql::Result<OffsetDateTime> {
        Ok(self
            .ops
            .added_at()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into())
    }

    async fn definition(&self) -> async_graphql::Result<GqlScheduleDefinition> {
        Ok(self
            .ops
            .definition()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into())
    }

    async fn cancelled_at(&self) -> async_graphql::Result<Option<OffsetDateTime>> {
        Ok(self
            .ops
            .cancelled_at()
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .map(Into::into))
    }

    async fn active_task(&self) -> async_graphql::Result<Option<Task>> {
        let ops = self
            .ops
            .active_task()
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(ops.map(|ops| Task {
            client: self.client.clone(),
            ops,
        }))
    }

    async fn tasks(&self, options: Option<GqlTaskListOptions>) -> async_graphql::Result<Vec<Task>> {
        let mut options = options.map(Tasks::from).unwrap_or_default();
        options.schedule_id = Some(self.ops.id());
        Ok(self
            .client
            .tasks(&options)
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into_iter()
            .map(|ops| Task {
                client: self.client.clone(),
                ops,
            })
            .collect())
    }
}

#[derive(Debug, Default, InputObject)]
#[graphql(name = "TaskListOptions")]
pub(crate) struct GqlTaskListOptions {
    /// Only return tasks with the given status.
    #[graphql(default)]
    pub(crate) include_status: Option<HashSet<GqlTaskStatus>>,
    /// Include only matching labels.
    #[graphql(default)]
    pub(crate) include_labels: Option<Vec<GqlLabelMatch>>,
    /// Only return tasks for a given schedule.
    #[graphql(default)]
    pub(crate) schedule_id: Option<Uuid>,
    /// A search text, how it is used is up to the store.
    #[graphql(default)]
    pub(crate) search: Option<String>,
    /// The way tasks are ordered before applying
    /// offset and limit.
    #[graphql(default)]
    pub(crate) order: GqlTaskListOrder,
    /// The task offset.
    #[graphql(default)]
    pub(crate) offset: u64,
    /// The maximum amount of tasks to fetch.
    #[graphql(default = 50)]
    pub(crate) limit: u64,
    /// Only return tasks with the given worker selector kind.
    #[graphql(default)]
    pub kind: Option<String>,
    #[graphql(default)]
    pub(crate) added_after: Option<OffsetDateTime>,
    #[graphql(default)]
    pub(crate) added_before: Option<OffsetDateTime>,
    #[graphql(default)]
    pub(crate) finished_after: Option<OffsetDateTime>,
    #[graphql(default)]
    pub(crate) finished_before: Option<OffsetDateTime>,
    #[graphql(default)]
    pub(crate) target_after: Option<OffsetDateTime>,
    #[graphql(default)]
    pub(crate) target_before: Option<OffsetDateTime>,
}

/// Allow matching on task labels.
#[derive(Debug, InputObject)]
#[graphql(name = "LabelMatch")]
pub(crate) struct GqlLabelMatch {
    /// The name of the label.
    pub(crate) label: String,
    /// The value of the label.
    pub(crate) exact_value: Option<Value>,
}

/// The ordering of returned tasks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Enum)]
#[graphql(name = "TaskListOrder")]
pub(crate) enum GqlTaskListOrder {
    AddedAsc,
    #[default]
    AddedDesc,
    TargetAsc,
    TargetDesc,
}

impl From<TaskListOrder> for GqlTaskListOrder {
    fn from(value: TaskListOrder) -> Self {
        match value {
            TaskListOrder::AddedAsc => Self::AddedAsc,
            TaskListOrder::AddedDesc => Self::AddedDesc,
            TaskListOrder::TargetAsc => Self::TargetAsc,
            TaskListOrder::TargetDesc => Self::TargetDesc,
        }
    }
}

impl From<GqlTaskListOrder> for TaskListOrder {
    fn from(value: GqlTaskListOrder) -> Self {
        match value {
            GqlTaskListOrder::AddedAsc => Self::AddedAsc,
            GqlTaskListOrder::AddedDesc => Self::AddedDesc,
            GqlTaskListOrder::TargetAsc => Self::TargetAsc,
            GqlTaskListOrder::TargetDesc => Self::TargetDesc,
        }
    }
}

impl From<GqlTaskListOptions> for Tasks {
    fn from(value: GqlTaskListOptions) -> Self {
        Self {
            include_status: value
                .include_status
                .map(|v| v.into_iter().map(Into::into).collect()),
            include_labels: value.include_labels.map(|labels| {
                labels
                    .into_iter()
                    .map(|label| LabelMatch {
                        label: label.label,
                        value: match label.exact_value {
                            Some(v) => LabelValueMatch::Value(v),
                            None => LabelValueMatch::AnyValue,
                        },
                    })
                    .collect()
            }),
            schedule_id: value.schedule_id,
            search: value.search,
            order: value.order.into(),
            offset: value.offset,
            limit: value.limit,
            kind: value.kind,
            added_after: value.added_after,
            added_before: value.added_before,
            finished_after: value.finished_after,
            finished_before: value.finished_before,
            target_after: value.target_after,
            target_before: value.target_before,
        }
    }
}

#[derive(Debug, Default, InputObject)]
#[graphql(name = "ScheduleListOptions")]
pub(crate) struct GqlScheduleListOptions {
    /// Only include active or inactive schedules.
    #[graphql(default)]
    active: Option<bool>,
    /// Include only matching labels.
    #[graphql(default)]
    pub(crate) include_labels: Option<Vec<GqlLabelMatch>>,
    /// The way tasks are ordered before applying
    /// offset and limit.
    #[graphql(default)]
    pub(crate) order: GqlScheduleListOrder,
    /// The task offset.
    #[graphql(default)]
    pub(crate) offset: u64,
    /// The maximum amount of tasks to fetch.
    #[graphql(default = 50)]
    pub(crate) limit: u64,
    /// Only return schedules with a known selector kind.
    #[graphql(default)]
    pub kind: Option<String>,
    /// A search text, how it is used is up to the store.
    #[graphql(default)]
    pub(crate) search: Option<String>,
    #[graphql(default)]
    pub(crate) added_after: Option<OffsetDateTime>,
    #[graphql(default)]
    pub(crate) added_before: Option<OffsetDateTime>,
    #[graphql(default)]
    pub(crate) cancelled_after: Option<OffsetDateTime>,
    #[graphql(default)]
    pub(crate) cancelled_before: Option<OffsetDateTime>,
}

impl From<GqlScheduleListOptions> for Schedules {
    fn from(value: GqlScheduleListOptions) -> Self {
        Self {
            active: value.active,
            include_labels: value.include_labels.map(|labels| {
                labels
                    .into_iter()
                    .map(|label| LabelMatch {
                        label: label.label,
                        value: match label.exact_value {
                            Some(v) => LabelValueMatch::Value(v),
                            None => LabelValueMatch::AnyValue,
                        },
                    })
                    .collect()
            }),
            kind: value.kind,
            order: value.order.into(),
            offset: value.offset,
            limit: value.limit,
            search: value.search,
            added_after: value.added_after,
            added_before: value.added_before,
            cancelled_after: value.cancelled_after,
            cancelled_before: value.cancelled_before,
        }
    }
}

/// The ordering of returned schedules.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Enum)]
#[graphql(name = "ScheduleListOrder")]
pub(crate) enum GqlScheduleListOrder {
    AddedAsc,
    #[default]
    AddedDesc,
}

impl From<ScheduleListOrder> for GqlScheduleListOrder {
    fn from(value: ScheduleListOrder) -> Self {
        match value {
            ScheduleListOrder::AddedAsc => Self::AddedAsc,
            ScheduleListOrder::AddedDesc => Self::AddedDesc,
        }
    }
}

impl From<GqlScheduleListOrder> for ScheduleListOrder {
    fn from(value: GqlScheduleListOrder) -> Self {
        match value {
            GqlScheduleListOrder::AddedAsc => Self::AddedAsc,
            GqlScheduleListOrder::AddedDesc => Self::AddedDesc,
        }
    }
}
