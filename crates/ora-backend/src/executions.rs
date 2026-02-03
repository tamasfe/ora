//! Executions and related types.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    executors::ExecutorId,
    jobs::{JobId, JobTypeId, RetryPolicy, TimeoutPolicy},
};

/// An execution ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ExecutionId(pub Uuid);

impl std::fmt::Display for ExecutionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ExecutionId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ExecutionId> for Uuid {
    fn from(value: ExecutionId) -> Self {
        value.0
    }
}

/// Details of an execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionDetails {
    /// The execution ID.
    pub id: ExecutionId,
    /// The executor ID handling the execution (if any).
    pub executor_id: Option<ExecutorId>,
    /// The status of the execution.
    pub status: ExecutionStatus,
    /// The timestamp when the execution was created.
    pub created_at: SystemTime,
    /// The timestamp when the execution started.
    pub started_at: Option<SystemTime>,
    /// The timestamp when the execution successfully completed.
    pub succeeded_at: Option<SystemTime>,
    /// The timestamp when the execution failed.
    pub failed_at: Option<SystemTime>,
    /// The timestamp when the execution was cancelled.
    pub cancelled_at: Option<SystemTime>,
    /// The output or result of the execution.
    pub output_json: Option<String>,
    /// The error message if the execution failed or was cancelled.
    pub failure_reason: Option<String>,
}

/// The status of an execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// The execution is pending.
    Pending,
    /// The execution is in progress.
    InProgress,
    /// The execution has completed successfully.
    Succeeded,
    /// The execution has failed.
    Failed,
    /// The execution was cancelled.
    Cancelled,
}

/// An execution ready to be executed.
#[derive(Debug, Clone)]
pub struct ReadyExecution {
    /// The execution ID.
    pub execution_id: ExecutionId,
    /// The job ID.
    pub job_id: JobId,
    /// The job type ID of the execution.
    pub job_type_id: JobTypeId,
    /// The input JSON for the execution.
    pub input_payload_json: String,
    /// The attempt number of the execution.
    ///
    /// The first attempt is 1.
    pub attempt_number: u64,
    /// The retry policy of the job this execution belongs to.
    pub retry_policy: RetryPolicy,
    /// The target execution time for the execution.
    pub target_execution_time: SystemTime,
}

/// An execution assigned to an executor.
pub struct StartedExecution {
    /// The execution ID.
    pub execution_id: ExecutionId,
    /// The executor ID.
    pub executor_id: ExecutorId,
    /// The timestamp when the execution started.
    pub started_at: SystemTime,
}

/// An execution that was successfully completed.
pub struct SucceededExecution {
    /// The execution ID.
    pub execution_id: ExecutionId,
    /// The timestamp when the execution completed.
    pub succeeded_at: SystemTime,
    /// The output JSON of the execution.
    pub output_json: String,
}

/// An execution that has failed.
pub struct FailedExecution {
    /// The execution ID.
    pub execution_id: ExecutionId,
    /// The job ID.
    pub job_id: JobId,
    /// The timestamp when the execution failed.
    pub failed_at: SystemTime,
    /// The error message for the failure.
    pub failure_reason: String,
}

/// An execution that is currently in progress.
pub struct InProgressExecution {
    /// The execution ID.
    pub execution_id: ExecutionId,
    /// The job ID.
    pub job_id: JobId,
    /// The executor ID handling the execution.
    pub executor_id: ExecutorId,
    /// The target execution time for the execution.
    pub target_execution_time: SystemTime,
    /// The time the execution was assigned to the executor.
    pub started_at: SystemTime,
    /// The timeout policy of the job this execution belongs to.
    pub timeout_policy: TimeoutPolicy,
    /// The retry policy of the job this execution belongs to.
    pub retry_policy: RetryPolicy,
    /// The attempt number of the execution.
    ///
    /// The first attempt is 1.
    pub attempt_number: u64,
}
