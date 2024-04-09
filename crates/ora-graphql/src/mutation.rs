use std::sync::Arc;

use async_graphql::{InputObject, Object, OneofObject};
use base64::Engine;
use ora_client::ClientOperations;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    common::{
        GqlScheduleDefinition, GqlTaskDataFormat, GqlTaskDefinition, GqlTimeoutPolicy,
        GqlWorkerRegistry, GqlWorkerSelector, Label,
    },
    query::{GqlScheduleListOptions, GqlTaskListOptions, Schedule, Task},
};

pub struct Mutation {
    pub(crate) client: Arc<dyn ClientOperations>,
    pub(crate) worker_registry: Arc<dyn GqlWorkerRegistry>,
}

#[Object]
impl Mutation {
    async fn add_task(&self, task: AddTaskInput) -> async_graphql::Result<Task> {
        let task = task.into_task_definition();

        let task_id = self
            .client
            .add_task(task.into())
            .await
            .map_err(async_graphql::Error::new_with_source)?;

        let ops = self
            .client
            .task(task_id)
            .await
            .map_err(async_graphql::Error::new_with_source)?;

        Ok(Task {
            client: self.client.clone(),
            ops,
            worker_registry: self.worker_registry.clone(),
        })
    }

    async fn cancel_task(&self, task_id: Uuid) -> async_graphql::Result<Task> {
        let task = self
            .client
            .task(task_id)
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        task.cancel()
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(Task {
            client: self.client.clone(),
            ops: task,
            worker_registry: self.worker_registry.clone(),
        })
    }

    async fn cancel_tasks(&self, options: GqlTaskListOptions) -> async_graphql::Result<Vec<Task>> {
        let ids = self
            .client
            .cancel_tasks(&options.into())
            .await
            .map_err(async_graphql::Error::new_with_source)?;

        Ok(self
            .client
            .tasks_by_ids(ids)
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into_iter()
            .map(|ops| Task {
                client: self.client.clone(),
                ops,
                worker_registry: self.worker_registry.clone(),
            })
            .collect())
    }

    async fn add_schedule(
        &self,
        schedule: GqlScheduleDefinition,
    ) -> async_graphql::Result<Schedule> {
        let schedule_id = self
            .client
            .add_schedule(schedule.into())
            .await
            .map_err(async_graphql::Error::new_with_source)?;

        let ops = self
            .client
            .schedule(schedule_id)
            .await
            .map_err(async_graphql::Error::new_with_source)?;

        Ok(Schedule {
            client: self.client.clone(),
            ops,
            worker_registry: self.worker_registry.clone(),
        })
    }

    async fn cancel_schedule(&self, schedule_id: Uuid) -> async_graphql::Result<Schedule> {
        let schedule = self
            .client
            .schedule(schedule_id)
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        schedule
            .cancel()
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(Schedule {
            client: self.client.clone(),
            ops: schedule,
            worker_registry: self.worker_registry.clone(),
        })
    }

    async fn cancel_schedules(
        &self,
        options: GqlScheduleListOptions,
    ) -> async_graphql::Result<Vec<Schedule>> {
        let ids = self
            .client
            .cancel_schedules(&options.into())
            .await
            .map_err(async_graphql::Error::new_with_source)?;

        Ok(self
            .client
            .schedules_by_ids(ids)
            .await
            .map_err(async_graphql::Error::new_with_source)?
            .into_iter()
            .map(|ops| Schedule {
                client: self.client.clone(),
                ops,
                worker_registry: self.worker_registry.clone(),
            })
            .collect())
    }
}

#[derive(OneofObject)]
enum AddTaskInput {
    Task(GqlTaskDefinition),
    JsonTask(InputJsonTaskDefinition),
}

impl AddTaskInput {
    fn into_task_definition(self) -> GqlTaskDefinition {
        match self {
            AddTaskInput::Task(task) => task,
            AddTaskInput::JsonTask(task) => GqlTaskDefinition {
                target: task.target,
                worker_selector: task.worker_selector,
                data_base64: base64::prelude::BASE64_STANDARD.encode(task.data.to_string()),
                data_format: GqlTaskDataFormat::Json,
                labels: task.labels,
                timeout: task.timeout,
            },
        }
    }
}

#[derive(InputObject)]
struct InputJsonTaskDefinition {
    /// The target time of the task execution.
    target: OffsetDateTime,
    /// The worker selector of the task.
    worker_selector: GqlWorkerSelector,
    /// The input data.
    #[graphql(default)]
    data: Value,
    /// Arbitrary task labels.
    #[graphql(default)]
    labels: Vec<Label>,
    /// An optional timeout policy.
    #[graphql(default)]
    timeout: GqlTimeoutPolicy,
}
