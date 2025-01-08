use std::time::{Duration, SystemTime};

use ora_storage::{IndexMap, JobExecutionStatus};
use rkyv::{Archive, Deserialize, Serialize};
use rusqlite::{
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, Value, ValueRef},
    ToSql,
};

/// Scheduling policy for a schedule.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub enum ScheduleJobTimingPolicy {
    /// A schedule that repeats.
    Repeat(SchedulingPolicyRepeat),
    /// A schedule based on a cron expression.
    Cron(SchedulingPolicyCron),
}

impl From<ora_storage::ScheduleJobTimingPolicy> for ScheduleJobTimingPolicy {
    fn from(value: ora_storage::ScheduleJobTimingPolicy) -> Self {
        match value {
            ora_storage::ScheduleJobTimingPolicy::Repeat(value) => {
                ScheduleJobTimingPolicy::Repeat(value.into())
            }
            ora_storage::ScheduleJobTimingPolicy::Cron(value) => {
                ScheduleJobTimingPolicy::Cron(value.into())
            }
        }
    }
}

impl From<ScheduleJobTimingPolicy> for ora_storage::ScheduleJobTimingPolicy {
    fn from(value: ScheduleJobTimingPolicy) -> Self {
        match value {
            ScheduleJobTimingPolicy::Repeat(value) => {
                ora_storage::ScheduleJobTimingPolicy::Repeat(value.into())
            }
            ScheduleJobTimingPolicy::Cron(value) => {
                ora_storage::ScheduleJobTimingPolicy::Cron(value.into())
            }
        }
    }
}

impl ToSql for ScheduleJobTimingPolicy {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Blob(serialize!(self).map_err(
            |err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)),
        )?)))
    }
}

impl FromSql for ScheduleJobTimingPolicy {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        deserialize_bytes!(ArchivedScheduleJobTimingPolicy, value.as_blob()?)
            .map_err(|err| FromSqlError::Other(Box::new(err)))
    }
}

/// Scheduling policy for a schedule that repeats.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct SchedulingPolicyRepeat {
    /// The interval between each job.
    pub interval: Duration,
    /// Whether the schedule should create a job immediately.
    pub immediate: bool,
    /// The policy for missed jobs.
    pub missed_policy: ScheduleMissedTimePolicy,
}

impl From<ora_storage::SchedulingPolicyRepeat> for SchedulingPolicyRepeat {
    fn from(value: ora_storage::SchedulingPolicyRepeat) -> Self {
        Self {
            interval: value.interval,
            immediate: value.immediate,
            missed_policy: value.missed_policy.into(),
        }
    }
}

impl From<SchedulingPolicyRepeat> for ora_storage::SchedulingPolicyRepeat {
    fn from(value: SchedulingPolicyRepeat) -> Self {
        Self {
            interval: value.interval,
            immediate: value.immediate,
            missed_policy: value.missed_policy.into(),
        }
    }
}

/// Scheduling policy based on a cron expression.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct SchedulingPolicyCron {
    /// The cron expression.
    pub cron_expression: String,
    /// Whether the schedule should create a job immediately.
    pub immediate: bool,
    /// The policy for missed jobs.
    pub missed_policy: ScheduleMissedTimePolicy,
}

impl From<ora_storage::SchedulingPolicyCron> for SchedulingPolicyCron {
    fn from(value: ora_storage::SchedulingPolicyCron) -> Self {
        Self {
            cron_expression: value.cron_expression,
            immediate: value.immediate,
            missed_policy: value.missed_policy.into(),
        }
    }
}

impl From<SchedulingPolicyCron> for ora_storage::SchedulingPolicyCron {
    fn from(value: SchedulingPolicyCron) -> Self {
        Self {
            cron_expression: value.cron_expression,
            immediate: value.immediate,
            missed_policy: value.missed_policy.into(),
        }
    }
}

/// Policy for missed jobs.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub enum ScheduleMissedTimePolicy {
    /// Skip any missed times.
    Skip,
    /// Create a job for each missed time.
    Create,
}

impl From<ora_storage::ScheduleMissedTimePolicy> for ScheduleMissedTimePolicy {
    fn from(value: ora_storage::ScheduleMissedTimePolicy) -> Self {
        match value {
            ora_storage::ScheduleMissedTimePolicy::Skip => ScheduleMissedTimePolicy::Skip,
            ora_storage::ScheduleMissedTimePolicy::Create => ScheduleMissedTimePolicy::Create,
        }
    }
}

impl From<ScheduleMissedTimePolicy> for ora_storage::ScheduleMissedTimePolicy {
    fn from(value: ScheduleMissedTimePolicy) -> Self {
        match value {
            ScheduleMissedTimePolicy::Skip => ora_storage::ScheduleMissedTimePolicy::Skip,
            ScheduleMissedTimePolicy::Create => ora_storage::ScheduleMissedTimePolicy::Create,
        }
    }
}

/// Policy for new jobs created by a schedule.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub enum ScheduleJobCreationPolicy {
    /// Create a new job from the given job definition.
    JobDefinition(ScheduleNewJobDefinition),
}

impl From<ora_storage::ScheduleJobCreationPolicy> for ScheduleJobCreationPolicy {
    fn from(value: ora_storage::ScheduleJobCreationPolicy) -> Self {
        match value {
            ora_storage::ScheduleJobCreationPolicy::JobDefinition(value) => {
                ScheduleJobCreationPolicy::JobDefinition(value.into())
            }
        }
    }
}

impl From<ScheduleJobCreationPolicy> for ora_storage::ScheduleJobCreationPolicy {
    fn from(value: ScheduleJobCreationPolicy) -> Self {
        match value {
            ScheduleJobCreationPolicy::JobDefinition(value) => {
                ora_storage::ScheduleJobCreationPolicy::JobDefinition(value.into())
            }
        }
    }
}

impl ToSql for ScheduleJobCreationPolicy {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Blob(serialize!(self).map_err(
            |err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)),
        )?)))
    }
}

impl FromSql for ScheduleJobCreationPolicy {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        deserialize_bytes!(ArchivedScheduleJobCreationPolicy, value.as_blob()?)
            .map_err(|err| FromSqlError::Other(Box::new(err)))
    }
}

/// A job definition for a new job created by a schedule.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct ScheduleNewJobDefinition {
    /// The job type ID.
    pub job_type_id: String,
    /// The input payload of the job.
    pub input_payload_json: String,
    /// Timeout policy for the job.
    pub timeout_policy: JobTimeoutPolicy,
    /// Retry policy for the job.
    pub retry_policy: JobRetryPolicy,
    /// Labels of the job.
    pub labels: IndexMap<String, String>,
}

impl From<ora_storage::ScheduleNewJobDefinition> for ScheduleNewJobDefinition {
    fn from(value: ora_storage::ScheduleNewJobDefinition) -> Self {
        Self {
            job_type_id: value.job_type_id,
            input_payload_json: value.input_payload_json,
            timeout_policy: value.timeout_policy.into(),
            retry_policy: value.retry_policy.into(),
            labels: value.labels,
        }
    }
}

impl From<ScheduleNewJobDefinition> for ora_storage::ScheduleNewJobDefinition {
    fn from(value: ScheduleNewJobDefinition) -> Self {
        Self {
            job_type_id: value.job_type_id,
            input_payload_json: value.input_payload_json,
            timeout_policy: value.timeout_policy.into(),
            retry_policy: value.retry_policy.into(),
            labels: value.labels,
        }
    }
}

/// Job timeout policy.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct JobTimeoutPolicy {
    /// The timeout in seconds.
    pub timeout: Option<Duration>,
    /// The base time for the timeout.
    ///
    /// The timeout is calculated from this time.
    pub base_time: JobTimeoutBaseTime,
}

impl From<ora_storage::JobTimeoutPolicy> for JobTimeoutPolicy {
    fn from(value: ora_storage::JobTimeoutPolicy) -> Self {
        Self {
            timeout: value.timeout,
            base_time: value.base_time.into(),
        }
    }
}

impl From<JobTimeoutPolicy> for ora_storage::JobTimeoutPolicy {
    fn from(value: JobTimeoutPolicy) -> Self {
        Self {
            timeout: value.timeout,
            base_time: value.base_time.into(),
        }
    }
}

impl ToSql for JobTimeoutPolicy {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Blob(serialize!(self).map_err(
            |err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)),
        )?)))
    }
}

impl FromSql for JobTimeoutPolicy {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        deserialize_bytes!(ArchivedJobTimeoutPolicy, value.as_blob()?)
            .map_err(|err| FromSqlError::Other(Box::new(err)))
    }
}

/// The base time for the timeout.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub enum JobTimeoutBaseTime {
    /// The base time is the start time of the job.
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

impl From<ora_storage::JobTimeoutBaseTime> for JobTimeoutBaseTime {
    fn from(value: ora_storage::JobTimeoutBaseTime) -> Self {
        match value {
            ora_storage::JobTimeoutBaseTime::StartTime => JobTimeoutBaseTime::StartTime,
            ora_storage::JobTimeoutBaseTime::TargetExecutionTime => {
                JobTimeoutBaseTime::TargetExecutionTime
            }
        }
    }
}

impl From<JobTimeoutBaseTime> for ora_storage::JobTimeoutBaseTime {
    fn from(value: JobTimeoutBaseTime) -> Self {
        match value {
            JobTimeoutBaseTime::StartTime => ora_storage::JobTimeoutBaseTime::StartTime,
            JobTimeoutBaseTime::TargetExecutionTime => {
                ora_storage::JobTimeoutBaseTime::TargetExecutionTime
            }
        }
    }
}

/// Job retry policy.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct JobRetryPolicy {
    /// The number of retries for the job.
    ///
    /// If the number of retries is zero, the job is not retried.
    pub retries: u64,
}

impl From<ora_storage::JobRetryPolicy> for JobRetryPolicy {
    fn from(value: ora_storage::JobRetryPolicy) -> Self {
        Self {
            retries: value.retries,
        }
    }
}

impl From<JobRetryPolicy> for ora_storage::JobRetryPolicy {
    fn from(value: JobRetryPolicy) -> Self {
        Self {
            retries: value.retries,
        }
    }
}

impl ToSql for JobRetryPolicy {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Blob(serialize!(self).map_err(
            |err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)),
        )?)))
    }
}

impl FromSql for JobRetryPolicy {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        deserialize_bytes!(ArchivedJobRetryPolicy, value.as_blob()?)
            .map_err(|err| FromSqlError::Other(Box::new(err)))
    }
}

pub(crate) struct SqlSystemTime(pub SystemTime);

impl From<SystemTime> for SqlSystemTime {
    fn from(value: SystemTime) -> Self {
        Self(value)
    }
}

impl From<SqlSystemTime> for SystemTime {
    fn from(value: SqlSystemTime) -> Self {
        value.0
    }
}

impl ToSql for SqlSystemTime {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Integer(
            i64::try_from(
                self.0
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
                    .as_nanos(),
            )
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?,
        )))
    }
}

impl FromSql for SqlSystemTime {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        Ok(SqlSystemTime(
            SystemTime::UNIX_EPOCH
                + Duration::from_nanos(
                    u64::try_from(value.as_i64()?)
                        .map_err(|err| FromSqlError::Other(Box::new(err)))?,
                ),
        ))
    }
}

// WHEN failed_at_unix_ns IS NOT NULL THEN 'failed'
// WHEN succeeded_at_unix_ns IS NOT NULL THEN 'succeeded'
// WHEN started_at_unix_ns IS NOT NULL THEN 'running'
// WHEN assigned_at_unix_ns IS NOT NULL THEN 'assigned'
// WHEN ready_at_unix_ns IS NOT NULL THEN 'ready'
// ELSE 'pending'

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub(crate) enum SqlExecutionStatus {
    Pending,
    Ready,
    Assigned,
    Running,
    Succeeded,
    Failed,
}

impl ToSql for SqlExecutionStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Integer((*self as u8).into())))
    }
}

impl FromSql for SqlExecutionStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_i64()? {
            0 => Ok(SqlExecutionStatus::Pending),
            1 => Ok(SqlExecutionStatus::Ready),
            2 => Ok(SqlExecutionStatus::Assigned),
            3 => Ok(SqlExecutionStatus::Running),
            4 => Ok(SqlExecutionStatus::Succeeded),
            5 => Ok(SqlExecutionStatus::Failed),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl From<JobExecutionStatus> for SqlExecutionStatus {
    fn from(value: JobExecutionStatus) -> Self {
        match value {
            JobExecutionStatus::Pending => SqlExecutionStatus::Pending,
            JobExecutionStatus::Ready => SqlExecutionStatus::Ready,
            JobExecutionStatus::Assigned => SqlExecutionStatus::Assigned,
            JobExecutionStatus::Running => SqlExecutionStatus::Running,
            JobExecutionStatus::Succeeded => SqlExecutionStatus::Succeeded,
            JobExecutionStatus::Failed => SqlExecutionStatus::Failed,
        }
    }
}

impl From<SqlExecutionStatus> for JobExecutionStatus {
    fn from(value: SqlExecutionStatus) -> Self {
        match value {
            SqlExecutionStatus::Pending => JobExecutionStatus::Pending,
            SqlExecutionStatus::Ready => JobExecutionStatus::Ready,
            SqlExecutionStatus::Assigned => JobExecutionStatus::Assigned,
            SqlExecutionStatus::Running => JobExecutionStatus::Running,
            SqlExecutionStatus::Succeeded => JobExecutionStatus::Succeeded,
            SqlExecutionStatus::Failed => JobExecutionStatus::Failed,
        }
    }
}
