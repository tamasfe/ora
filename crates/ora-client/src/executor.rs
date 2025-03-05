//! An implementation of a job executor.

use std::{num::NonZero, sync::Arc, time::SystemTime};

use async_trait::async_trait;
use eyre::bail;
use ora_proto::server::v1::executor_service_client::ExecutorServiceClient;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;
use uuid::Uuid;

use crate::job_type::{JobType, JobTypeExt, JobTypeMetadata};

mod run;

pub use eyre::Result;

/// Options for configuring an executor.
#[derive(Debug, Clone)]
pub struct ExecutorOptions {
    /// The name of the executor.
    pub name: String,
    /// The maximum number of concurrent executions.
    ///
    /// Defaults to 1.
    pub max_concurrent_executions: NonZero<u32>,
    /// The grace period for job cancellations,
    /// after which the futures will be dropped.
    pub cancellation_grace_period: std::time::Duration,
}

impl Default for ExecutorOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            max_concurrent_executions: NonZero::new(1).unwrap(),
            cancellation_grace_period: std::time::Duration::from_secs(30),
        }
    }
}

/// An executor for running jobs.
pub struct Executor<C = Channel> {
    options: ExecutorOptions,
    client: ExecutorServiceClient<C>,
    handlers: Vec<Arc<dyn ExecutionHandlerRaw + Send + Sync>>,
}

impl<C> Executor<C> {
    /// Create a new executor.
    pub fn new(client: ExecutorServiceClient<C>) -> Self {
        Self::with_options(client, ExecutorOptions::default())
    }

    /// Create a new executor with the given options.
    pub fn with_options(client: ExecutorServiceClient<C>, options: ExecutorOptions) -> Self {
        Self {
            client,
            options,
            handlers: Vec::new(),
        }
    }

    /// Add a new handler to the executor.
    ///
    /// # Panics
    ///
    /// Panics if a handler for the same job type is already registered.
    pub fn add_handler(&mut self, handler: Arc<dyn ExecutionHandlerRaw + Send + Sync>) {
        assert!(
            !self
                .handlers
                .iter()
                .any(|h| h.job_type_metadata().id == handler.job_type_metadata().id),
            "A handler for job type {} is already registered",
            handler.job_type_metadata().id
        );

        self.handlers.push(handler);
    }

    /// Try to add a new handler to the executor.
    ///
    /// If a handler for the same job type is already registered,
    /// this function will return an error.
    pub fn try_add_handler(
        &mut self,
        handler: Arc<dyn ExecutionHandlerRaw + Send + Sync>,
    ) -> eyre::Result<()> {
        if self
            .handlers
            .iter()
            .any(|h| h.job_type_metadata().id == handler.job_type_metadata().id)
        {
            bail!(
                "A handler for job type {} is already registered",
                handler.job_type_metadata().id
            );
        }

        self.handlers.push(handler);
        Ok(())
    }
}

/// The context in which a job is executed.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    execution_id: Uuid,
    job_id: Uuid,
    target_execution_time: SystemTime,
    attempt_number: u64,
    job_type_id: String,
    cancellation_token: CancellationToken,
}

impl ExecutionContext {
    /// Get the ID of the current execution.
    #[must_use]
    pub fn execution_id(&self) -> Uuid {
        self.execution_id
    }

    /// Get the ID of the job.
    #[must_use]
    pub fn job_id(&self) -> Uuid {
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
    #[must_use]
    pub fn job_type_id(&self) -> &str {
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

/// An execution handler for a specific job type.
#[async_trait]
pub trait ExecutionHandler<J>
where
    J: JobType,
{
    /// Execute the given job execution.
    async fn execute(&self, context: ExecutionContext, input: J) -> eyre::Result<J::Output>;

    /// Return a raw handler to be used by an executor.
    fn raw_handler(self) -> Arc<dyn ExecutionHandlerRaw + Send + Sync>
    where
        Self: Sized + Send + Sync + 'static,
    {
        struct H<J, F>(F, std::marker::PhantomData<J>, JobTypeMetadata);

        #[async_trait]
        impl<J, F> ExecutionHandlerRaw for H<J, F>
        where
            J: JobType,
            F: ExecutionHandler<J> + Send + Sync + 'static,
        {
            fn can_execute(&self, context: &ExecutionContext) -> bool {
                context.job_type_id == J::id()
            }

            async fn execute(
                &self,
                context: ExecutionContext,
                input_json: &str,
            ) -> Result<String, String> {
                let input = serde_json::from_str::<J>(input_json)
                    .map_err(|e| format!("Failed to parse job input JSON: {e}"))?;

                let result = self
                    .0
                    .execute(context, input)
                    .await
                    .map_err(|e| format!("{e:?}"))?;

                let output_json = serde_json::to_string(&result)
                    .map_err(|e| format!("Failed to serialize job output JSON: {e}"))?;

                Ok(output_json)
            }

            fn job_type_metadata(&self) -> &JobTypeMetadata {
                &self.2
            }
        }

        Arc::new(H(self, std::marker::PhantomData, J::metadata()))
    }
}

#[async_trait]
impl<J, F, Fut> ExecutionHandler<J> for F
where
    J: JobType,
    F: Fn(ExecutionContext, J) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = eyre::Result<J::Output>> + Send + 'static,
{
    async fn execute(&self, context: ExecutionContext, input: J) -> eyre::Result<J::Output> {
        self(context, input).await
    }
}

/// A handler for executing jobs.
#[async_trait]
pub trait ExecutionHandlerRaw {
    /// Returns whether the handler can execute the
    /// given job execution.
    fn can_execute(&self, context: &ExecutionContext) -> bool;

    /// Execute the given job execution.
    ///
    /// The Ok variant must be a valid JSON,
    /// while the Err variant must be an error message of any kind.
    ///
    /// Note that while the input and outputs should be JSON,
    /// this might not be enforced by either the executor or the server.
    async fn execute(&self, context: ExecutionContext, input_json: &str) -> Result<String, String>;

    /// Get information about the job type this handler can execute.
    fn job_type_metadata(&self) -> &JobTypeMetadata;
}

/// A helper blanket trait for types that might implement [`ExecutionHandler`]
/// for multiple [`JobType`]s.
pub trait IntoExecutionHandler: Sized + Send + Sync + 'static {
    /// Convert `self` into a [`RawHandler`] that can be registered
    /// in workers.
    fn handler<J>(self) -> Arc<dyn ExecutionHandlerRaw + Send + Sync>
    where
        Self: ExecutionHandler<J>,
        J: JobType,
    {
        <Self as ExecutionHandler<J>>::raw_handler(self)
    }
}

impl<W> IntoExecutionHandler for W where W: Sized + Send + Sync + 'static {}
