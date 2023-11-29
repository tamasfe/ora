use async_graphql::async_trait::async_trait;
use ora_client::ClientOperations;
use ora_graphql::create_schema;
use ora_worker::registry::noop::NoopWorkerRegistry;

fn main() {
    println!("{}", create_schema(DummyClient, NoopWorkerRegistry).sdl());
}

#[derive(Debug)]
struct DummyClient;

#[async_trait]
#[allow(dead_code, unused)]
impl ClientOperations for DummyClient {
    async fn add_task(&self, task: ora_common::task::TaskDefinition) -> eyre::Result<uuid::Uuid> {
        todo!()
    }

    async fn task(
        &self,
        task_id: uuid::Uuid,
    ) -> eyre::Result<std::sync::Arc<dyn ora_client::TaskOperations>> {
        todo!()
    }

    async fn tasks(
        &self,
        options: &ora_client::Tasks,
    ) -> eyre::Result<Vec<std::sync::Arc<dyn ora_client::TaskOperations>>> {
        todo!()
    }

    async fn task_count(&self, options: &ora_client::Tasks) -> eyre::Result<u64> {
        todo!()
    }

    async fn task_labels(&self) -> eyre::Result<Vec<String>> {
        todo!()
    }

    async fn task_kinds(&self) -> eyre::Result<Vec<String>> {
        todo!()
    }

    async fn add_schedule(
        &self,
        schedule: ora_common::schedule::ScheduleDefinition,
    ) -> eyre::Result<uuid::Uuid> {
        todo!()
    }

    async fn schedule(
        &self,
        schedule_id: uuid::Uuid,
    ) -> eyre::Result<std::sync::Arc<dyn ora_client::ScheduleOperations>> {
        todo!()
    }

    async fn schedules(
        &self,
        options: &ora_client::Schedules,
    ) -> eyre::Result<Vec<std::sync::Arc<dyn ora_client::ScheduleOperations>>> {
        todo!()
    }

    async fn schedule_count(&self, options: &ora_client::Schedules) -> eyre::Result<u64> {
        todo!()
    }

    async fn schedule_labels(&self) -> eyre::Result<Vec<String>> {
        todo!()
    }
}
