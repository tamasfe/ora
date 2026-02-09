//! Ora executor implementation.

use std::{pin::Pin, sync::Arc, time::SystemTime};

use eyre::Context;
use schemars::Schema;
use tokio_util::sync::CancellationToken;
use wgroup::WaitGroup;

use crate::{
    execution::ExecutionId,
    job::JobId,
    job_type::{JobType, JobTypeId},
    proto::executors::v1::execution_service_client::ExecutionServiceClient,
};

mod capabilities;
mod connection;
mod executions;
mod heartbeat;
mod main_loop;

/// The error type for executor handlers.
pub type HandlerError = eyre::Report;

/// The result type for executor handlers.
pub type HandlerResult<T> = Result<T, HandlerError>;

/// Executor options.
pub struct ExecutorOptions {
    /// The name of the executor.
    pub name: String,
    /// The grace period for job cancellations,
    /// after which the pending futures will be simply dropped.
    pub cancellation_grace_period: std::time::Duration,
}

impl Default for ExecutorOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            cancellation_grace_period: std::time::Duration::from_secs(30),
        }
    }
}

/// An executor that executes scheduled jobs.
#[must_use = "an executor does nothing until it is started"]
pub struct Executor<C> {
    options: ExecutorOptions,
    client: ExecutionServiceClient<C>,
    on_execution_failed: Option<ExecutionFailedCb>,
    queues: Vec<ExecutorJobQueue>,
}

impl<C> Executor<C> {
    /// Create a new executor with default options and the given gRPC client.
    pub fn new(client: ExecutionServiceClient<C>) -> Self {
        Self::new_with_options(ExecutorOptions::default(), client)
    }

    /// Create a new executor with the given options and gRPC client.
    pub fn new_with_options(options: ExecutorOptions, client: ExecutionServiceClient<C>) -> Self {
        Self {
            options,
            client,
            on_execution_failed: None,
            queues: Vec::new(),
        }
    }

    /// Set the callback to be called when a job execution fails.
    ///
    /// Only one callback can be set,
    /// subsequent calls will overwrite any previous ones.
    pub fn on_execution_failed<F>(&mut self, cb: F)
    where
        F: Fn(ExecutionContext, &str) + Send + Sync + 'static,
    {
        self.on_execution_failed = Some(Arc::new(cb));
    }

    /// Get the options of the executor.
    pub fn options(&self) -> &ExecutorOptions {
        &self.options
    }

    /// Set the name of the executor.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.options.name = name.into();
        self
    }

    /// Add a handler for the given job type
    /// with default options.
    ///
    /// Any previous handler for the same job type will be replaced
    /// with the given one.
    pub fn handler<F, J, Fut>(self, handler: F) -> Self
    where
        F: Fn(ExecutionContext, J) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<J::Output>> + Send + 'static,
        J: JobType,
    {
        self.handler_with_options(handler, HandlerOptions::default())
    }

    /// Add a handler for the given job type with the given
    /// options.
    ///
    /// Any previous handler for the same job type will be replaced
    /// with the given one.
    pub fn handler_with_options<F, J, Fut>(mut self, handler: F, options: HandlerOptions) -> Self
    where
        F: Fn(ExecutionContext, J) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<J::Output>> + Send + 'static,
        J: JobType,
    {
        let job_type_id = J::job_type_id();

        self.queues
            .retain(|q| q.job_type_id.as_str() != job_type_id.as_str());

        let handler = Arc::new(handler);
        let handler: HandlerFn = Arc::new(move |ctx: ExecutionContext, payload_json: String| {
            Box::pin({
                let handler = handler.clone();
                async move {
                    let payload: J = serde_json::from_str(&payload_json)
                        .wrap_err("invalid payload for execution")?;
                    let output = handler(ctx, payload).await?;
                    let output_json = serde_json::to_string(&output)
                        .wrap_err("failed to serialize handler output")?;
                    Ok(output_json)
                }
            })
        });

        let input_schema = schemars::schema_for!(J);
        let output_schema = schemars::schema_for!(J::Output);

        let description = input_schema
            .as_object()
            .and_then(|o| o.get("description"))
            .and_then(|s| s.as_str())
            .map(Into::into);

        self.queues.push(ExecutorJobQueue {
            max_concurrent_jobs: options.max_concurrent,
            job_type_id,
            input_schema,
            output_schema,
            description,
            handler,
        });

        self
    }
}

/// A handle to a running executor.
///
/// The executor is stopped once this handle is dropped.
#[must_use = "The executor stops when this handle is dropped."]
pub struct ExecutorHandle {
    _wg: Option<WaitGroup>,
}

/// The context in which a job is executed.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    execution_id: ExecutionId,
    job_id: JobId,
    target_execution_time: SystemTime,
    attempt_number: u64,
    job_type_id: JobTypeId,
    cancellation_token: CancellationToken,
}

impl ExecutionContext {
    /// Get the ID of the current execution.
    #[must_use]
    pub fn execution_id(&self) -> ExecutionId {
        self.execution_id
    }

    /// Get the ID of the job.
    #[must_use]
    pub fn job_id(&self) -> JobId {
        self.job_id
    }

    /// Get the target execution time of the job.
    #[must_use]
    pub fn target_execution_time(&self) -> SystemTime {
        self.target_execution_time
    }

    /// Get the attempt number of the job.
    ///
    /// The first attempt has number 1.
    #[must_use]
    pub fn attempt_number(&self) -> u64 {
        self.attempt_number
    }

    /// The job type of the current job.
    pub fn job_type_id(&self) -> &JobTypeId {
        &self.job_type_id
    }

    /// Wait for the execution to be cancelled.
    pub async fn cancelled(&self) {
        self.cancellation_token.cancelled().await;
    }

    /// Check if the execution has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }
}

/// Options for a handler.
#[derive(Clone)]
pub struct HandlerOptions {
    /// The maximum number of concurrent executions for this handler.
    /// Cannot be lower than 1 (default).
    ///
    /// Note that setting this value too high
    /// may lead to resource exhaustion.
    pub max_concurrent: u64,
}

impl Default for HandlerOptions {
    fn default() -> Self {
        Self { max_concurrent: 1 }
    }
}

type ExecutionFailedCb = Arc<dyn Fn(ExecutionContext, &str) + Send + Sync>;

/// A job queue for the executor.
struct ExecutorJobQueue {
    /// The maximum number of concurrent jobs.
    max_concurrent_jobs: u64,
    /// The job type ID handled by this queue.
    job_type_id: JobTypeId,
    /// Input schema of the job type.
    input_schema: Schema,
    /// Output schema of the job type.
    output_schema: Schema,
    /// Description of the job of the job type.
    description: Option<String>,
    /// The handler for the job type.
    handler: HandlerFn,
}

type HandlerFn = Arc<
    dyn Fn(ExecutionContext, String) -> Pin<Box<dyn Future<Output = HandlerResult<String>> + Send>>
        + Send
        + Sync,
>;
