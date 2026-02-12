//! Job and related types.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    common::{Label, LabelFilter, TimeRange},
    executions::{ExecutionDetails, ExecutionId, ExecutionStatus},
    executors::ExecutorId,
    schedules::ScheduleId,
};

/// A job ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct JobId(pub Uuid);

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for JobId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<JobId> for Uuid {
    fn from(value: JobId) -> Self {
        value.0
    }
}

/// A Case-sensitive identifier for a job type.
///
/// The ID must conform to the following rules:
///
/// - An ID is made up of segments separated by dots (`.`).
/// - Each segment must start with a letter (A-Z, a-z) and can be followed by
///   letters, digits (0-9), or underscores (`_`).
/// - The ID must not start or end with a dot, and must not contain consecutive dots.
///
/// Examples of valid IDs:
/// - `data_processing`
/// - `image.resize_v2`
/// - `myService.analytics.GenerateReport`
///
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct JobTypeId(String);

impl std::fmt::Display for JobTypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl JobTypeId {
    /// Create a new job type ID.
    pub fn new<S: Into<String>>(s: S) -> Result<Self, InvalidJobTypeId> {
        let s = s.into();

        if !Self::validate(&s) {
            return Err(InvalidJobTypeId(s));
        }

        Ok(Self(s))
    }

    /// Create a job type ID without validation.
    ///
    pub fn new_unchecked<S: Into<String>>(s: S) -> Self {
        Self(s.into())
    }

    fn validate(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }

        let segments = s.split('.');

        for segment in segments {
            if segment.is_empty() {
                return false;
            }

            let mut chars = segment.bytes();

            match chars.next() {
                Some(c) if c.is_ascii_alphabetic() => {}
                _ => return false,
            }

            for c in chars {
                if !(c.is_ascii_alphanumeric() || c == b'_') {
                    return false;
                }
            }
        }

        true
    }

    /// Get the job type ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return the inner string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// An error indicating that a job type ID is invalid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidJobTypeId(pub String);

impl std::fmt::Display for InvalidJobTypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"invalid job type ID: "{}""#, self.0)
    }
}

impl core::error::Error for InvalidJobTypeId {}

/// A job type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobType {
    /// The ID of the job type.
    pub id: JobTypeId,
    /// An optional description of the job type.
    pub description: Option<String>,
    /// An optional input JSON schema.
    pub input_schema_json: Option<String>,
    /// An optional output JSON schema.
    pub output_schema_json: Option<String>,
}

/// A job definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDefinition {
    /// The job type ID.
    pub job_type_id: JobTypeId,
    /// The target execution time.
    pub target_execution_time: SystemTime,
    /// The job input payload JSON.
    pub input_payload_json: String,
    /// Labels associated with the job.
    pub labels: Vec<Label>,
    /// The timeout policy for the job.
    pub timeout_policy: TimeoutPolicy,
    /// The retry policy for the job.
    pub retry_policy: RetryPolicy,
}

/// Details of a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDetails {
    /// The job ID.
    pub id: JobId,
    /// The job definition.
    pub job: JobDefinition,
    /// The creation time of the job.
    pub created_at: SystemTime,
    /// The executions of the job.
    pub executions: Vec<ExecutionDetails>,
    /// The schedule the job belongs to.
    pub schedule_id: Option<ScheduleId>,
}

/// Timeout policy for a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutPolicy {
    /// The timeout duration.
    ///
    /// If zero, the job has no timeout.
    pub timeout: Duration,
    /// The base time of the timeout.
    pub base_time: TimeoutBaseTime,
}

/// The base time of the timeout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimeoutBaseTime {
    /// The timeout is measured from the job's target execution time.
    TargetExecutionTime,
    /// The timeout is measured from the actual start time of the job.
    StartTime,
}

/// Retry policy for a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// The maximum number of retries.
    ///
    /// If zero, the job will not be retried.
    pub retries: u64,
}

/// Filters for querying jobs.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct JobFilters {
    /// Filter by job IDs.
    pub job_ids: Option<Vec<JobId>>,
    /// Filter by job type IDs.
    pub job_type_ids: Option<Vec<JobTypeId>>,
    /// Filter by executor IDs.
    pub executor_ids: Option<Vec<ExecutorId>>,
    /// Filter by execution IDs.
    pub execution_ids: Option<Vec<ExecutionId>>,
    /// Execution status.
    pub execution_statuses: Option<Vec<ExecutionStatus>>,
    /// Filter by target execution time range.
    pub target_execution_time: Option<TimeRange>,
    /// Filter by creation time range.
    pub created_at: Option<TimeRange>,
    /// Filter by labels.
    pub labels: Option<Vec<LabelFilter>>,
    /// Filter by schedule IDs.
    pub schedule_ids: Option<Vec<ScheduleId>>,
}

/// The ordering options for listing jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobOrderBy {
    /// Order by target execution time ascending.
    TargetExecutionTimeAsc,
    /// Order by target execution time descending.
    TargetExecutionTimeDesc,
    /// Order by creation time ascending.
    CreatedAtAsc,
    /// Order by creation time descending.
    CreatedAtDesc,
}

/// A new job.
pub struct NewJob {
    /// The job definition.
    pub job: JobDefinition,
    /// The schedule the job should belong to.
    pub schedule_id: Option<ScheduleId>,
}

/// A job that was cancelled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledJob {
    /// The job ID.
    pub job_id: JobId,
    /// The last execution ID of the job.
    pub last_execution_id: ExecutionId,
}

/// The result of adding jobs,
/// indicating whether the jobs were added or already existed.
pub enum AddedJobs {
    /// The jobs were added successfully.
    Added(Vec<JobId>),
    /// No jobs were added because existing jobs matched the `if_not_exists` filters.
    Existing(Vec<JobId>),
}

impl AddedJobs {
    /// Get the IDs of the added or existing jobs.
    #[must_use]
    pub fn job_ids(&self) -> &[JobId] {
        match self {
            AddedJobs::Added(ids) | AddedJobs::Existing(ids) => ids,
        }
    }

    /// Return the added job IDs, or an empty slice if no jobs were added.
    #[must_use]
    pub fn added_job_ids(&self) -> &[JobId] {
        match self {
            AddedJobs::Added(ids) => ids,
            AddedJobs::Existing(_) => &[],
        }
    }
}
