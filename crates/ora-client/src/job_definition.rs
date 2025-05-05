//! Job definitions and related types.

use std::{borrow::Cow, time::SystemTime};

use eyre::{Context, OptionExt};
use ora_proto::{
    common::v1::{JobRetryPolicy, JobTimeoutPolicy},
    server::v1::{Job, JobExecutionStatus},
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    schedule_definition::{
        ScheduleDefinition, ScheduleJobTimingPolicy, ScheduleJobTimingPolicyCron,
        ScheduleJobTimingPolicyRepeat,
    },
    IndexMap,
};

/// The definition of a job that can be executed
/// in the future.
#[derive(Debug, Clone)]
#[must_use]
pub struct JobDefinition {
    /// The ID of the job type.
    pub job_type_id: Cow<'static, str>,
    /// The target execution time of the job.
    ///
    /// If not provided, it should be set to the current time.
    pub target_execution_time: std::time::SystemTime,
    /// The job input payload JSON that is passed to the worker.
    pub input_payload_json: String,
    /// The labels of the job.
    pub labels: IndexMap<String, String>,
    /// The timeout policy of the job.
    pub timeout_policy: TimeoutPolicy,
    /// Retry policy for the job.
    pub retry_policy: RetryPolicy,
    /// Additional metadata for the job.
    pub metadata_json: Option<String>,
}

impl JobDefinition {
    /// Set the target execution time of the job.
    pub fn at(mut self, target_execution_time: impl Into<std::time::SystemTime>) -> Self {
        self.target_execution_time = target_execution_time.into();
        self
    }

    /// Set the target execution time of the job to the current time.
    pub fn now(mut self) -> Self {
        self.target_execution_time = std::time::SystemTime::now();
        self
    }

    /// Add a label to the job.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Add a JSON label to the job.
    ///
    /// # Panics
    ///
    /// If the value cannot be serialized to JSON.
    pub fn with_label_json(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        self.labels.insert(
            key.into(),
            serde_json::to_string(&value).expect("label serialization failed"),
        );
        self
    }

    /// Set the timeout for the job.
    ///
    /// The timeout is calculated from the start time of the job
    /// by default.
    pub fn with_timeout(mut self, timeout: impl Into<std::time::Duration>) -> Self {
        self.timeout_policy.timeout = Some(timeout.into());
        self
    }

    /// Set the number of retries for the job.
    pub fn with_retries(mut self, retries: u64) -> Self {
        self.retry_policy.retries = retries;
        self
    }

    /// Set additional metadata for the job.
    ///
    /// Note that this will replace any existing metadata.
    ///
    /// # Panics
    ///
    /// If the metadata cannot be serialized to JSON.
    pub fn replace_metadata(mut self, metadata: impl Serialize) -> Self {
        self.metadata_json =
            Some(serde_json::to_string(&metadata).expect("metadata serialization failed"));
        self
    }
}

impl JobDefinition {
    /// Create a schedule from the job definition
    /// that repeats at a fixed interval.
    pub fn repeat_every(self, interval: std::time::Duration) -> ScheduleDefinition {
        ScheduleDefinition {
            job_timing_policy: ScheduleJobTimingPolicy::Repeat(ScheduleJobTimingPolicyRepeat {
                interval,
                immediate: false,
                missed_time_policy: Default::default(),
            }),
            job_creation_policy:
                crate::schedule_definition::ScheduleJobCreationPolicy::JobDefinition(self),
            time_range: None,
            labels: Default::default(),
            metadata_json: None,
            propagate_labels_to_jobs: true,
        }
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
        let expr = cron_expression.into();

        let mut parse_options = cronexpr::ParseOptions::default();
        parse_options.fallback_timezone_option = cronexpr::FallbackTimezoneOption::UTC;

        cronexpr::parse_crontab_with(&expr, parse_options)?;

        Ok(ScheduleDefinition {
            job_timing_policy: ScheduleJobTimingPolicy::Cron(ScheduleJobTimingPolicyCron {
                cron_expression: expr,
                immediate: false,
                missed_time_policy: Default::default(),
            }),
            job_creation_policy:
                crate::schedule_definition::ScheduleJobCreationPolicy::JobDefinition(self),
            time_range: None,
            labels: Default::default(),
            metadata_json: None,
            propagate_labels_to_jobs: true,
        })
    }
}

/// A timeout policy for a job.
#[derive(Debug, Default, Clone, Copy)]
pub struct TimeoutPolicy {
    /// The timeout for the job.
    pub timeout: Option<std::time::Duration>,
    /// The base time for the timeout.
    ///
    /// The timeout is calculated from this time.
    pub base_time: TimeoutBaseTime,
}

/// The base time for the timeout.
#[derive(Debug, Default, Clone, Copy)]
pub enum TimeoutBaseTime {
    /// The base time is the start time of the job.
    #[default]
    StartTime,
    /// The base time is the target execution time of the job.
    ///
    /// Note that if the target execution time is not set,
    /// the timeout is calculated from the start time of the job.
    ///
    /// If the target execution time is in the past,
    /// the jobs may be immediately timed out.
    TargetExecutionTime,
}

/// A retry policy for a job.
#[derive(Debug, Default, Clone, Copy)]
pub struct RetryPolicy {
    /// The number of retries for the job.
    ///
    /// If the number of retries is zero, the job is not retried.
    pub retries: u64,
}

/// The status of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobStatus {
    /// The job is not yet scheduled for execution.
    Pending,
    /// The job is ready to be executed.
    Ready,
    /// The job is assigned to an executor but has not started yet.
    Assigned,
    /// The job is currently being executed.
    Running,
    /// The job has completed successfully.
    Succeeded,
    /// The job has failed.
    Failed,
}

impl JobStatus {
    /// Check if the status is terminal.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed)
    }

    /// Returns `true` if the job status is [`Pending`].
    ///
    /// [`Pending`]: JobStatus::Pending
    #[must_use]
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// Returns `true` if the job status is [`Ready`].
    ///
    /// [`Ready`]: JobStatus::Ready
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Returns `true` if the job status is [`Assigned`].
    ///
    /// [`Assigned`]: JobStatus::Assigned
    #[must_use]
    pub fn is_assigned(&self) -> bool {
        matches!(self, Self::Assigned)
    }

    /// Returns `true` if the job status is [`Running`].
    ///
    /// [`Running`]: JobStatus::Running
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Returns `true` if the job status is [`Succeeded`].
    ///
    /// [`Succeeded`]: JobStatus::Succeeded
    #[must_use]
    pub fn is_succeeded(&self) -> bool {
        matches!(self, Self::Succeeded)
    }

    /// Returns `true` if the job status is [`Failed`].
    ///
    /// [`Failed`]: JobStatus::Failed
    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }
}

impl From<JobExecutionStatus> for JobStatus {
    fn from(status: JobExecutionStatus) -> Self {
        match status {
            JobExecutionStatus::Pending | JobExecutionStatus::Unspecified => Self::Pending,
            JobExecutionStatus::Ready => Self::Ready,
            JobExecutionStatus::Assigned => Self::Assigned,
            JobExecutionStatus::Running => Self::Running,
            JobExecutionStatus::Succeeded => Self::Succeeded,
            JobExecutionStatus::Failed => Self::Failed,
        }
    }
}

impl From<JobStatus> for JobExecutionStatus {
    fn from(status: JobStatus) -> Self {
        match status {
            JobStatus::Pending => JobExecutionStatus::Pending,
            JobStatus::Ready => JobExecutionStatus::Ready,
            JobStatus::Assigned => JobExecutionStatus::Assigned,
            JobStatus::Running => JobExecutionStatus::Running,
            JobStatus::Succeeded => JobExecutionStatus::Succeeded,
            JobStatus::Failed => JobExecutionStatus::Failed,
        }
    }
}

/// All core information about a job.
#[derive(Debug, Clone)]
pub struct JobDetails {
    /// The unique identifier of the job.
    pub id: Uuid,
    /// Whether the job is active.
    ///
    /// Inactive jobs are not scheduled for execution.
    ///
    /// Jobs become inactive when they succeed or fail all their retries,
    /// or get cancelled.
    pub active: bool,
    /// Whether the job was cancelled.
    pub cancelled: bool,
    /// The ID of the job type.
    pub job_type_id: String,
    /// The target execution time of the job.
    ///
    /// If not provided, it should be set to the current time.
    pub target_execution_time: SystemTime,
    /// The job input payload JSON that is passed to the executor.
    pub input_payload_json: String,
    /// The labels of the job.
    pub labels: IndexMap<String, String>,
    /// The timeout policy of the job.
    pub timeout_policy: JobTimeoutPolicy,
    /// Retry policy for the job.
    pub retry_policy: JobRetryPolicy,
    /// The creation time of the job.
    pub created_at: SystemTime,
    /// A list of executions for the job.
    pub executions: Vec<ExecutionDetails>,
}

impl JobDetails {
    /// Get the status of the job.
    #[must_use]
    pub fn status(&self) -> JobStatus {
        let Some(last_execution) = self.executions.last() else {
            return JobStatus::Pending;
        };

        last_execution.status
    }
}

impl TryFrom<Job> for JobDetails {
    type Error = eyre::Report;

    fn try_from(value: Job) -> Result<Self, Self::Error> {
        let Some(definition) = value.definition else {
            return Err(eyre::eyre!("missing job definition"));
        };

        Ok(Self {
            id: value.id.parse().wrap_err("invalid job ID")?,
            active: value.active,
            cancelled: value.cancelled,
            job_type_id: definition.job_type_id,
            target_execution_time: definition
                .target_execution_time
                .and_then(|t| t.try_into().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH),
            input_payload_json: definition.input_payload_json,
            labels: definition
                .labels
                .into_iter()
                .map(|job_label| (job_label.key, job_label.value))
                .collect(),
            timeout_policy: definition.timeout_policy.unwrap_or_default(),
            retry_policy: definition.retry_policy.unwrap_or_default(),
            created_at: value
                .created_at
                .map(TryInto::try_into)
                .transpose()?
                .ok_or_eyre("missing creation time")?,
            executions: value
                .executions
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        })
    }
}

/// All core information about an execution
/// that is associated with a job.
#[derive(Debug, Clone)]
pub struct ExecutionDetails {
    /// The ID of the job execution.
    pub id: Uuid,
    /// The ID of the job.
    pub job_id: Uuid,
    /// The ID of the associated executor.
    pub executor_id: Option<Uuid>,
    /// The status of the job execution.
    pub status: JobStatus,
    /// The time the job execution was created.
    pub created_at: SystemTime,
    /// The time the job execution was marked as ready.
    pub ready_at: Option<SystemTime>,
    /// The time the job execution was assigned to an executor.
    pub assigned_at: Option<SystemTime>,
    /// The time the job execution has started.
    pub started_at: Option<SystemTime>,
    /// The time the job execution has succeeded.
    pub succeeded_at: Option<SystemTime>,
    /// The time the job execution has failed.
    pub failed_at: Option<SystemTime>,
    /// The output payload of the execution.
    pub output_payload_json: Option<String>,
    /// The error message of the execution.
    pub failure_reason: Option<String>,
}

impl TryFrom<ora_proto::server::v1::JobExecution> for ExecutionDetails {
    type Error = eyre::Report;

    fn try_from(value: ora_proto::server::v1::JobExecution) -> Result<Self, Self::Error> {
        Ok(Self {
            status: value.status().into(),
            id: value.id.parse().wrap_err("invalid execution ID")?,
            job_id: value.job_id.parse().wrap_err("invalid job ID")?,
            executor_id: value
                .executor_id
                .map(|id| id.parse())
                .transpose()
                .wrap_err("invalid executor ID")?,
            created_at: value
                .created_at
                .map(TryInto::try_into)
                .transpose()?
                .ok_or_eyre("missing creation time")?,
            ready_at: value.ready_at.map(TryInto::try_into).transpose()?,
            assigned_at: value.assigned_at.map(TryInto::try_into).transpose()?,
            started_at: value.started_at.map(TryInto::try_into).transpose()?,
            succeeded_at: value.succeeded_at.map(TryInto::try_into).transpose()?,
            failed_at: value.failed_at.map(TryInto::try_into).transpose()?,
            output_payload_json: value.output_payload_json,
            failure_reason: value.failure_reason,
        })
    }
}
