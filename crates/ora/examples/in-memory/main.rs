use async_trait::async_trait;
use ora::{Handler, IntoHandler, MemoryStore, Scheduler, Task, Worker};
use ora_api::client::Client;
use ora_common::timeout::TimeoutPolicy;
use ora_worker::TaskContext;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use tracing_subscriber::EnvFilter;

/// A task that echoes its input value and the elapsed
/// seconds until a handler receives the task.
#[derive(Serialize, Deserialize)]
struct ExampleTask {
    /// The value that should be echoed back by the handler.
    input: Value,
    /// We need to pass the task's target, as it is not
    /// available in the handler function at the moment.
    target: OffsetDateTime,
}

/// The output of [`ExampleTask`].
#[derive(Serialize, Deserialize)]
struct ExampleTaskOutput {
    input_value: Value,
    delay_seconds: f64,
}

/// Implement [`Task`], where we specify the output type.
///
/// The trait provides some default values, such as timeout.
impl Task for ExampleTask {
    type Output = ExampleTaskOutput;

    // We also set a default timeout policy after which
    // incomplete tasks should fail, by default tasks will never timeout.
    fn timeout() -> TimeoutPolicy {
        TimeoutPolicy::from_target(Duration::seconds(1))
    }
}

struct ExampleWorker;

/// A handler implementation for the the example task type.
#[async_trait]
impl Handler<ExampleTask> for ExampleWorker {
    async fn run(
        &self,
        ctx: TaskContext,
        task: ExampleTask,
    ) -> eyre::Result<<ExampleTask as Task>::Output> {
        tracing::info!(task_id = %ctx.task_id(), "worker received task");
        let now = OffsetDateTime::now_utc();
        Ok(ExampleTaskOutput {
            input_value: task.input,
            delay_seconds: (now - task.target).as_seconds_f64(),
        })
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive("info".parse().unwrap())
                .from_env()
                .unwrap(),
        )
        .init();

    // Create an in-memory store with the default options.
    let store = MemoryStore::new();

    // Also expose the store in the browser (or to applications).
    tokio::spawn(run_graphql_server(store.clone()));

    // Create a scheduler that will time the executions of added tasks.
    let scheduler = Scheduler::new(store.clone());

    // Spawn it onto a background task.
    let scheduler_handle = tokio::spawn(scheduler.run());

    // Create a worker that is capable of executing our tasks.
    //
    // In this case the worker accesses the store directly and is
    // unaware of other workers, two workers with the same type (worker selector)
    // will both receive the tasks and will race for completion.
    let mut worker: Worker<MemoryStore> = Worker::new(store.clone());

    // Register our handler onto the worker for our example task type.
    //
    // Only one handler can be registered with a type (worker selector).
    //
    // We also cannot add more handlers after the worker has started.
    //
    // Remove this line to watch our example task time out.
    worker.register_handler(ExampleWorker.handler::<ExampleTask>());

    // Run the worker in the backround as well.
    let worker_handle = tokio::spawn(worker.run());

    // Schedule a task at 2 seconds from now.
    let target = OffsetDateTime::now_utc() + Duration::seconds(2);
    let example_input = json!("hello from 2 seconds ago!");

    // After spawning the task, we get a handle to it.
    let task = store
        .add_task(
            ExampleTask {
                input: example_input.clone(),
                target,
            }
            .task()
            .at(target),
        )
        .await?;

    tracing::info!(task_id = %task.id(), "task spawned");

    // The handle implements `Future`, so we can wait for the output.
    let output = task.await?;
    assert_eq!(example_input, output.input_value);
    tracing::info!(scheduling_delay = output.delay_seconds, "task completed");

    // Wait forever.
    tokio::select! {
        res = worker_handle => {
            res.unwrap().unwrap();
        }
        res = scheduler_handle => {
            res.unwrap().unwrap();
        }
    }

    Ok(())
}

// Expose the store via graphql on `127.0.0.1:8080`.
async fn run_graphql_server(store: MemoryStore) {
    use async_graphql::http::GraphiQLSource;
    use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
    use axum::{
        extract::Extension,
        response::{self, IntoResponse},
        routing::get,
        Router, Server,
    };
    use ora_graphql::{create_schema, OraSchema};

    async fn graphql_handler(schema: Extension<OraSchema>, req: GraphQLRequest) -> GraphQLResponse {
        schema.execute(req.into_inner()).await.into()
    }

    async fn graphiql() -> impl IntoResponse {
        response::Html(GraphiQLSource::build().endpoint("/").finish())
    }

    let schema = create_schema(store);

    let app = Router::new()
        .route("/", get(graphiql).post(graphql_handler))
        .layer(Extension(schema));

    tracing::info!(
        endpoint = "http://localhost:8080",
        "running graphql interface"
    );

    Server::bind(&"127.0.0.1:8080".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
