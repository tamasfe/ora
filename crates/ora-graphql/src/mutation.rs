use std::sync::Arc;

use async_graphql::Object;
use ora_client::ClientOperations;
use uuid::Uuid;

use crate::{
    common::{GqlScheduleDefinition, GqlTaskDefinition},
    query::{Schedule, Task},
};

#[derive(Debug)]
pub struct Mutation {
    pub(crate) client: Arc<dyn ClientOperations>,
}

#[Object]
impl Mutation {
    async fn add_task(&self, task: GqlTaskDefinition) -> async_graphql::Result<Task> {
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
        })
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
        })
    }

    async fn cancel_schedule(&self, id: Uuid) -> async_graphql::Result<Schedule> {
        let schedule = self
            .client
            .schedule(id)
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        schedule
            .cancel()
            .await
            .map_err(async_graphql::Error::new_with_source)?;
        Ok(Schedule {
            client: self.client.clone(),
            ops: schedule,
        })
    }
}
