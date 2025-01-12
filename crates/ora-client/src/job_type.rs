//! Traits and types that allow defining jobs in a mostly type-safe way.

use std::{borrow::Cow, time::SystemTime};

use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    job_definition::{JobDefinition, RetryPolicy, TimeoutPolicy},
    schedule_definition::ScheduleDefinition,
    IndexMap,
};

/// A job type that can be executed by the server.
///
/// The instances of the type implementing this
/// are also used as inputs of the job.
pub trait JobType: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static {
    /// The output type of the job.
    type Output: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static;

    /// A unique identifier for the job type.
    ///
    /// Executors will primarily use this
    /// to determine which job type to execute.
    fn id() -> &'static str;

    /// A default timeout policy for the job type.
    #[must_use]
    fn default_timeout_policy() -> TimeoutPolicy {
        TimeoutPolicy::default()
    }

    /// A default retry policy for the job type.
    #[must_use]
    fn default_retry_policy() -> RetryPolicy {
        RetryPolicy::default()
    }

    /// Default labels for the job type.
    #[must_use]
    fn default_labels() -> IndexMap<String, String> {
        IndexMap::default()
    }
}

/// Extension trait for job types.
pub trait JobTypeExt: JobType {
    /// Create a new job definition for this job type.
    /// The target execution time is set to the current time.
    ///
    /// # Panics
    ///
    /// Panics if the job type cannot be serialized.
    fn job(&self) -> TypedJobDefinition<Self>;

    /// Return metadata for the job type.
    #[must_use]
    fn metadata() -> JobTypeMetadata;
}

impl<J> JobTypeExt for J
where
    J: JobType,
{
    fn job(&self) -> TypedJobDefinition<Self> {
        JobDefinition {
            job_type_id: Self::id().into(),
            target_execution_time: SystemTime::now(),
            input_payload_json: serde_json::to_string(self).unwrap(),
            labels: Self::default_labels(),
            timeout_policy: Self::default_timeout_policy(),
            retry_policy: Self::default_retry_policy(),
            metadata_json: None,
        }
        .into()
    }

    fn metadata() -> JobTypeMetadata {
        let input_schema = schemars::schema_for!(J);

        JobTypeMetadata {
            id: Self::id().into(),
            name: input_schema
                .schema
                .metadata
                .as_ref()
                .and_then(|m| m.title.clone()),
            description: input_schema
                .schema
                .metadata
                .as_ref()
                .and_then(|m| m.description.clone()),
            input_schema_json: Some(serde_json::to_string(&input_schema).unwrap()),
            output_schema_json: Some(
                serde_json::to_string(&schemars::schema_for!(J::Output)).unwrap(),
            ),
        }
    }
}

/// A job definition that carries the job type.
///
/// Note that this type does not guarantee that the job type
/// matches the underlying job definition.
#[derive(Debug, Clone)]
#[must_use]
pub struct TypedJobDefinition<J = ()> {
    inner: JobDefinition,
    _job_type: std::marker::PhantomData<fn() -> J>,
}

impl<J> TypedJobDefinition<J> {
    /// Set the target execution time of the job.
    pub fn at(mut self, target_execution_time: impl Into<std::time::SystemTime>) -> Self {
        self.inner.target_execution_time = target_execution_time.into();
        self
    }

    /// Set the target execution time of the job to the current time.
    pub fn now(mut self) -> Self {
        self.inner.target_execution_time = std::time::SystemTime::now();
        self
    }

    /// Add a label to the job.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.labels.insert(key.into(), value.into());
        self
    }

    /// Set the timeout for the job.
    ///
    /// The timeout is calculated from the start time of the job
    /// by default.
    pub fn with_timeout(mut self, timeout: impl Into<std::time::Duration>) -> Self {
        self.inner.timeout_policy.timeout = Some(timeout.into());
        self
    }

    /// Set the number of retries for the job.
    pub fn with_retries(mut self, retries: u64) -> Self {
        self.inner.retry_policy.retries = retries;
        self
    }

    /// Get the inner job definition.
    pub fn into_inner(self) -> JobDefinition {
        self.inner
    }

    /// Convert the job type to a different job type.
    pub fn cast_type<T>(self) -> TypedJobDefinition<T> {
        TypedJobDefinition {
            inner: self.inner,
            _job_type: std::marker::PhantomData,
        }
    }
}

impl<J> TypedJobDefinition<J> {
    /// Create a schedule from the job definition
    /// that repeats at a fixed interval.
    pub fn repeat_every(self, interval: std::time::Duration) -> ScheduleDefinition {
        self.inner.repeat_every(interval)
    }

    /// Create a schedule from the job definition
    /// that repeats according to a cron expression.
    ///
    /// # Errors
    ///
    /// If the cron expression is invalid.
    pub fn repeat_cron(
        self,
        cron_expression: impl Into<String>,
    ) -> eyre::Result<ScheduleDefinition> {
        self.inner.repeat_cron(cron_expression)
    }
}

impl<J> TypedJobDefinition<J>
where
    J: JobType,
{
    /// Get the input of the job, which is an instance of the job type.
    ///
    /// # Panics
    ///
    /// Panics if the input payload cannot be deserialized.
    #[must_use]
    pub fn input(&self) -> J {
        serde_json::from_str(&self.inner.input_payload_json).unwrap()
    }

    /// Try to get the input of the job, which is an instance of the job type.
    pub fn try_input(&self) -> Result<J, serde_json::Error> {
        serde_json::from_str(&self.inner.input_payload_json)
    }

    /// Set the input of the job.
    ///
    /// # Panics
    ///
    /// Panics if the input cannot be serialized.
    pub fn with_input(mut self, input: impl AsRef<J>) -> Self {
        self.inner.input_payload_json = serde_json::to_string(input.as_ref()).unwrap();
        self
    }

    /// Determine whether the job definition is valid
    /// for the job type.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.inner.job_type_id == J::id() && self.try_input().is_ok()
    }
}

impl<J> From<JobDefinition> for TypedJobDefinition<J> {
    fn from(job_definition: JobDefinition) -> Self {
        Self {
            inner: job_definition,
            _job_type: std::marker::PhantomData,
        }
    }
}

impl<J> From<TypedJobDefinition<J>> for JobDefinition {
    fn from(typed: TypedJobDefinition<J>) -> Self {
        typed.inner
    }
}

impl JobDefinition {
    /// Cast the job definition to a typed job definition
    /// with an unknown job type.
    pub fn into_typed_unknown(self) -> TypedJobDefinition {
        TypedJobDefinition {
            inner: self,
            _job_type: std::marker::PhantomData,
        }
    }

    /// Cast the job definition to a typed job definition
    /// with a known job type.
    pub fn into_typed<J>(self) -> TypedJobDefinition<J> {
        TypedJobDefinition {
            inner: self,
            _job_type: std::marker::PhantomData,
        }
    }
}

/// Metadata for a job type.
#[derive(Debug, Clone)]
pub struct JobTypeMetadata {
    /// The ID of the job type.
    pub id: Cow<'static, str>,
    /// The name of the job type.
    pub name: Option<String>,
    /// The description of the job type.
    pub description: Option<String>,
    /// The input JSON schema of the job type.
    pub input_schema_json: Option<String>,
    /// The output JSON schema of the job type.
    pub output_schema_json: Option<String>,
}
