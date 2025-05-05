//! Storage interface for the Ora server.

use async_trait::async_trait;
use futures::stream::BoxStream;
use ora_proto::{
    common::{
        self,
        v1::{
            self, schedule_job_creation_policy::JobCreation, schedule_job_timing_policy,
            JobDefinition, TimeRange,
        },
    },
    server::{self, v1::Job},
    snapshot,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use tonic::Status;
use uuid::Uuid;

/// Re-export the storage types.
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, ahash::RandomState>;
/// Re-export the storage types.
pub type IndexSet<T> = indexmap::IndexSet<T, ahash::RandomState>;

/// An interface for storing and querying job and schedule data used
/// by the Ora server.
#[async_trait]
pub trait Storage: Send + Sync + 'static + Clone {
    /// Add or update a job type.
    async fn job_types_added(&self, job_types: Vec<JobType>) -> eyre::Result<()>;
    /// Persist the given new jobs.
    async fn jobs_added(&self, jobs: Vec<NewJob>) -> eyre::Result<()>;
    /// Persist the given job if no jobs match the filter.
    ///
    /// If any jobs match the filter,
    /// return the first one in arbitrary order.
    async fn job_added_conditionally(
        &self,
        job: NewJob,
        filters: JobQueryFilters,
    ) -> eyre::Result<ConditionalJobResult>;
    /// Cancel the given jobs.
    ///
    /// - Mark the job as cancelled.
    /// - Mark the job as unschedulable.
    ///
    /// Returns the cancelled jobs with their active executions, if any.
    async fn jobs_cancelled(
        &self,
        job_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<Vec<CancelledJob>>;
    /// Add new executions.
    async fn executions_added(
        &self,
        executions: Vec<NewExecution>,
        timestamp: SystemTime,
    ) -> eyre::Result<()>;
    /// An execution is ready to be executed.
    async fn executions_ready(
        &self,
        execution_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<()>;
    /// An execution was assigned to an executor.
    async fn execution_assigned(
        &self,
        execution_id: Uuid,
        executor_id: Uuid,
        timestamp: SystemTime,
    ) -> eyre::Result<()>;
    /// Set an execution as started.
    async fn execution_started(
        &self,
        execution_id: Uuid,
        timestamp: SystemTime,
    ) -> eyre::Result<()>;
    /// Set an execution as succeeded.
    async fn execution_succeeded(
        &self,
        execution_id: Uuid,
        timestamp: SystemTime,
        output_payload_json: String,
    ) -> eyre::Result<()>;
    /// Set an execution as failed, if `mark_job_unschedulable` is false
    /// the job is not marked as unschedulable, this happens when the job
    /// is retried.
    async fn executions_failed(
        &self,
        execution_ids: &[Uuid],
        timestamp: SystemTime,
        reason: String,
        mark_job_unschedulable: bool,
    ) -> eyre::Result<()>;

    /// Return assigned executions that are not assigned to any of
    /// the given executor IDs.
    async fn orphan_execution_ids(&self, executor_ids: &[Uuid]) -> eyre::Result<Vec<Uuid>>;

    /// Mark a job as unschedulable.
    ///
    /// No more executions will be created for the jobs and the given IDs must not be
    /// returned by the `pending_jobs` method. However existing executions are not affected.
    async fn jobs_unschedulable(&self, job_ids: &[Uuid], timestamp: SystemTime)
        -> eyre::Result<()>;
    /// Executions that are not yet ready to be executed.
    ///
    /// The returned values must be in ascending order,
    /// an optional `after` parameter can be used to filter out values
    /// that were created before the given UUID.
    ///
    /// If the `after` parameter is given, the stream must not include
    /// the execution with the given UUID and any IDs that are smaller.
    ///
    /// The implementation may choose to return a limited
    /// number of executions in a single call in order to avoid
    /// resource exhaustion.
    async fn pending_executions(&self, after: Option<Uuid>) -> eyre::Result<Vec<PendingExecution>>;
    /// Unassigned executions that are ready at the time of the call.
    ///
    /// The returned values must be in ascending order,
    /// an optional `after` parameter can be used to filter out values
    /// that were created before the given UUID.
    ///
    /// The returned executions must be ordered
    /// by their ID in ascending order.
    ///
    /// The implementation may choose to return a limited
    /// number of executions in a single call in order to avoid
    /// resource exhaustion.
    async fn ready_executions(&self, after: Option<Uuid>) -> eyre::Result<Vec<ReadyExecution>>;
    /// Jobs that satisfy the following conditions:
    ///
    /// - have no executions in progress
    /// - are active
    ///
    /// The returned values must be in ascending order,
    /// an optional `after` parameter can be used to filter out values
    /// that were created before the given UUID.
    ///
    /// The implementation may choose to return a limited
    /// number of executions in a single call in order to avoid
    /// resource exhaustion.
    async fn pending_jobs(&self, after: Option<Uuid>) -> eyre::Result<Vec<PendingJob>>;

    /// Query and return a list of jobs with the given parameters.
    ///
    /// A next token can be provided to continue the query from the last result.
    async fn query_jobs(
        &self,
        cursor: Option<String>,
        limit: usize,
        order: JobQueryOrder,
        filters: JobQueryFilters,
    ) -> eyre::Result<JobQueryResult>;

    /// Query and return a list of job IDs with the given parameters.
    async fn query_job_ids(&self, filters: JobQueryFilters) -> eyre::Result<Vec<Uuid>>;

    /// Count the number of jobs that satisfy the given filters.
    async fn count_jobs(&self, filters: JobQueryFilters) -> eyre::Result<u64>;

    /// Query and return a list of job types with the given parameters.
    async fn query_job_types(&self) -> eyre::Result<Vec<JobType>>;

    /// Remove jobs and all related data, returns the removed job IDs.
    ///
    /// This should also remove all executions created by the jobs.
    async fn delete_jobs(&self, filters: JobQueryFilters) -> eyre::Result<Vec<Uuid>>;

    /// Persist the given new schedules.
    async fn schedules_added(&self, schedules: Vec<NewSchedule>) -> eyre::Result<()>;

    /// Persist the given schedule if no schedules match the filter.
    ///
    /// If any schedules match the filter,
    /// return the first one in arbitrary order.
    async fn schedule_added_conditionally(
        &self,
        schedule: NewSchedule,
        filters: ScheduleQueryFilters,
    ) -> eyre::Result<ConditionalScheduleResult>;

    /// Cancel the given schedules.
    ///
    /// - Mark the schedule as cancelled.
    /// - Mark the schedule as unschedulable.
    ///
    /// Returns the cancelled schedules.
    async fn schedules_cancelled(
        &self,
        schedule_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<Vec<CancelledSchedule>>;

    /// Mark the given schedules as unschedulable.
    async fn schedules_unschedulable(
        &self,
        schedule_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<()>;

    /// Return all pending schedules.
    ///
    /// A pending schedule is a schedule that
    /// satisfies all the following conditions:
    /// - is active
    /// - has not been cancelled
    /// - has no active jobs
    ///
    /// The returned values must be in ascending order,
    /// an optional `after` parameter can be used to filter out values
    /// that were created before the given UUID.
    ///
    /// The implementation may choose to return a limited
    /// number of executions in a single call in order to avoid
    /// resource exhaustion.
    async fn pending_schedules(&self, after: Option<Uuid>) -> eyre::Result<Vec<PendingSchedule>>;

    /// Query and return a list of schedules with the given parameters.
    ///
    /// A next token can be provided to continue the query from the last result.
    async fn query_schedules(
        &self,
        cursor: Option<String>,
        limit: usize,
        filters: ScheduleQueryFilters,
        order: ScheduleQueryOrder,
    ) -> eyre::Result<ScheduleQueryResult>;

    /// Query and return a list of schedule IDs with the given parameters.
    async fn query_schedule_ids(&self, filters: ScheduleQueryFilters) -> eyre::Result<Vec<Uuid>>;

    /// Count the number of schedules that satisfy the given filters.
    async fn count_schedules(&self, filters: ScheduleQueryFilters) -> eyre::Result<u64>;

    /// Remove schedules and all related data, returns the removed schedule IDs.
    ///
    /// This should also remove all jobs created by the schedules.
    async fn delete_schedules(&self, filters: ScheduleQueryFilters) -> eyre::Result<Vec<Uuid>>;
}

/// A trait for storages that support
/// exporting and importing snapshots of their data.
#[async_trait]
pub trait StorageSnapshot {
    /// Export a snapshot of the storage.
    fn export_snapshot(&self) -> BoxStream<'static, eyre::Result<snapshot::v1::SnapshotData>>;

    /// Import a snapshot of the storage.
    ///
    /// The snapshot stream must be consumed to completion.
    ///
    /// Whether data is overwritten or merged is up to the implementation.
    async fn import_snapshot(
        &self,
        snapshot: BoxStream<'static, eyre::Result<snapshot::v1::SnapshotData>>,
    ) -> eyre::Result<()>;
}

/// Essential data for a job.
#[derive(Debug, Clone)]
pub struct NewJob {
    /// The unique identifier of the job.
    pub id: Uuid,
    /// The schedule ID of the job.
    pub schedule_id: Option<Uuid>,
    /// The time the job was created.
    pub created_at: SystemTime,
    /// The job type ID.
    pub job_type_id: String,
    /// The target execution time of the job.
    pub target_execution_time: SystemTime,
    /// The input payload of the job.
    pub input_payload_json: String,
    /// Timeout policy for the job.
    pub timeout_policy: JobTimeoutPolicy,
    /// Retry policy for the job.
    pub retry_policy: JobRetryPolicy,
    /// Labels of the job.
    pub labels: IndexMap<String, String>,
    /// Arbitrary metadata in JSON format.
    pub metadata_json: Option<String>,
}

/// A job that was added conditionally.
pub enum ConditionalJobResult {
    /// The job was added successfully.
    Added,
    /// A matching job already exists.
    AlreadyExists {
        /// The ID of the existing job.
        job_id: Uuid,
    },
}

/// A pending job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingJob {
    /// The unique identifier of the job.
    pub id: Uuid,
    /// The target execution time of the job.
    pub target_execution_time: SystemTime,
    /// The number of previous executions.
    pub execution_count: u64,
    /// The retry policy for the job.
    pub retry_policy: JobRetryPolicy,
    /// The timeout policy for the job.
    pub timeout_policy: JobTimeoutPolicy,
}

/// Job timeout policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JobTimeoutPolicy {
    /// The timeout in seconds.
    pub timeout: Option<Duration>,
    /// The base time for the timeout.
    ///
    /// The timeout is calculated from this time.
    pub base_time: JobTimeoutBaseTime,
}

impl From<v1::JobTimeoutPolicy> for JobTimeoutPolicy {
    fn from(proto: v1::JobTimeoutPolicy) -> Self {
        Self {
            timeout: proto.timeout.and_then(|d| d.try_into().ok()),
            base_time: proto.base_time().into(),
        }
    }
}

impl From<JobTimeoutPolicy> for v1::JobTimeoutPolicy {
    fn from(policy: JobTimeoutPolicy) -> Self {
        Self {
            timeout: policy.timeout.and_then(|d| d.try_into().ok()),
            base_time: v1::JobTimeoutBaseTime::from(policy.base_time).into(),
        }
    }
}

impl From<v1::JobTimeoutBaseTime> for JobTimeoutBaseTime {
    fn from(proto: v1::JobTimeoutBaseTime) -> Self {
        match proto {
            v1::JobTimeoutBaseTime::TargetExecutionTime => Self::TargetExecutionTime,
            v1::JobTimeoutBaseTime::StartTime | v1::JobTimeoutBaseTime::Unspecified => {
                Self::StartTime
            }
        }
    }
}

impl From<JobTimeoutBaseTime> for v1::JobTimeoutBaseTime {
    fn from(base_time: JobTimeoutBaseTime) -> Self {
        match base_time {
            JobTimeoutBaseTime::StartTime => Self::StartTime,
            JobTimeoutBaseTime::TargetExecutionTime => Self::TargetExecutionTime,
        }
    }
}

/// The base time for the timeout.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum JobTimeoutBaseTime {
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

/// Job retry policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JobRetryPolicy {
    /// The number of retries for the job.
    ///
    /// If the number of retries is zero, the job is not retried.
    pub retries: u64,
}

impl From<v1::JobRetryPolicy> for JobRetryPolicy {
    fn from(proto: v1::JobRetryPolicy) -> Self {
        Self {
            retries: proto.retries,
        }
    }
}

impl From<JobRetryPolicy> for v1::JobRetryPolicy {
    fn from(policy: JobRetryPolicy) -> Self {
        Self {
            retries: policy.retries,
        }
    }
}

/// A new pending execution.
#[derive(Debug, Clone)]
pub struct NewExecution {
    /// The unique identifier of the execution.
    pub id: Uuid,
    /// The unique identifier of the job.
    pub job_id: Uuid,
    /// Attempt number of the execution.
    pub attempt_number: u64,
    /// The target execution time of the job.
    pub target_execution_time: SystemTime,
}

/// A new pending execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingExecution {
    /// The unique identifier of the execution.
    pub id: Uuid,
    /// The target execution time of the job.
    pub target_execution_time: SystemTime,
}

/// An execution that is ready to be executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyExecution {
    /// The unique identifier of the execution.
    pub id: Uuid,
    /// The unique identifier of the job.
    pub job_id: Uuid,
    /// The input payload of the job.
    pub input_payload_json: String,
    /// Attempt number of the execution.
    pub attempt_number: u64,
    /// The job type ID.
    pub job_type_id: String,
    /// The target execution time of the job.
    pub target_execution_time: SystemTime,
    /// Timeout policy for the job.
    pub timeout_policy: JobTimeoutPolicy,
}

/// A job type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobType {
    /// The ID of the job type.
    pub id: String,
    /// The name of the job type.
    pub name: String,
    /// The description of the job type.
    pub description: String,
    /// The input schema of the job type.
    pub input_schema_json: Option<String>,
    /// The output schema of the job type.
    pub output_schema_json: Option<String>,
}

/// Filters for querying jobs.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct JobQueryFilters {
    /// Job IDs to filter the query results by.
    pub job_ids: Option<IndexSet<Uuid>>,
    /// Job type IDs to filter the query results by.
    pub job_type_ids: Option<IndexSet<String>>,
    /// Execution IDs to filter the query results by.
    pub execution_ids: Option<IndexSet<Uuid>>,
    /// Schedule IDs to filter the query results by.
    pub schedule_ids: Option<IndexSet<Uuid>>,
    /// Execution status to filter the query results by.
    pub execution_status: Option<IndexSet<JobExecutionStatus>>,
    /// Labels to filter the query results by.
    pub labels: Option<IndexMap<String, JobLabelFilterValue>>,
    /// Whether to return active or inactive jobs only.
    pub active: Option<bool>,
    /// Jobs created after the given time, inclusive.
    pub created_after: Option<SystemTime>,
    /// Jobs created before the given time, exclusive.
    pub created_before: Option<SystemTime>,
    /// Jobs with target execution time after the given time, inclusive.
    pub target_execution_time_after: Option<SystemTime>,
    /// Jobs with target execution time before the given time, exclusive.
    pub target_execution_time_before: Option<SystemTime>,
}

/// The order of jobs returned.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum JobQueryOrder {
    /// Order by the time the job was created in ascending order.
    CreatedAtAsc,
    /// Order by the time the job was created in descending order.
    CreatedAtDesc,
    /// Order by the target execution time in ascending order.
    TargetExecutionTimeAsc,
    /// Order by the target execution time in descending order.
    TargetExecutionTimeDesc,
}

/// The results of a job query.
#[derive(Debug, Clone)]
pub struct JobQueryResult {
    /// The jobs that satisfy the query.
    pub jobs: Vec<JobDetails>,
    /// A cursor that can be used to continue the query.
    pub cursor: Option<String>,
    /// Whether there are more results to query.
    pub has_more: bool,
}

/// Job status used for querying jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobExecutionStatus {
    /// The job execution is pending and is not ready to run.
    Pending,
    /// The job execution is ready to run.
    Ready,
    /// The job execution is assigned to an executor.
    Assigned,
    /// The job execution is running.
    Running,
    /// The job execution is completed successfully.
    Succeeded,
    /// The job execution is failed.
    Failed,
}

/// Job label filter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobLabelFilterValue {
    /// The label exists with any value.
    Exists,
    /// The label does not exist.
    Equals(String),
}

/// All core information about a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDetails {
    /// Whether the job is active.
    pub active: bool,
    /// Whether the job was cancelled.
    pub cancelled: bool,
    /// The unique identifier of the job.
    pub id: Uuid,
    /// The ID of the job type.
    pub job_type_id: String,
    /// The schedule ID of the job.
    pub schedule_id: Option<Uuid>,
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
    /// Arbitrary metadata in JSON format.
    pub metadata_json: Option<String>,
}

/// All core information about an execution
/// that is associated with a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionDetails {
    /// The ID of the job execution.
    pub id: Uuid,
    /// The ID of the job.
    pub job_id: Uuid,
    /// The ID of the associated executor.
    pub executor_id: Option<Uuid>,
    /// The status of the job execution.
    pub status: JobExecutionStatus,
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

/// A job that was cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelledJob {
    /// The unique identifier of the job.
    pub id: Uuid,
    /// Active execution of the job.
    pub active_execution: Option<Uuid>,
}

/// A new schedule.
#[derive(Debug, Clone)]
pub struct NewSchedule {
    /// The unique identifier of the schedule.
    pub id: Uuid,
    /// The time the schedule was created.
    pub created_at: SystemTime,
    /// Scheduling policy for the schedule.
    pub job_timing_policy: ScheduleJobTimingPolicy,
    /// Policy for new jobs created by the schedule.
    pub job_creation_policy: ScheduleJobCreationPolicy,
    /// Labels of the job.
    pub labels: IndexMap<String, String>,
    /// The time range for the schedule.
    pub time_range: Option<ScheduleTimeRange>,
    /// Arbitrary metadata in JSON format.
    pub metadata_json: Option<String>,
}

/// Conditionally added schedule result.
pub enum ConditionalScheduleResult {
    /// The schedule was added successfully.
    Added,
    /// A matching schedule already exists.
    AlreadyExists {
        /// The ID of the existing schedule.
        schedule_id: Uuid,
    },
}

/// Scheduling policy for a schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleJobTimingPolicy {
    /// A schedule that repeats.
    Repeat(SchedulingPolicyRepeat),
    /// A schedule based on a cron expression.
    Cron(SchedulingPolicyCron),
}

/// Scheduling policy for a schedule that repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingPolicyRepeat {
    /// The interval between each job.
    pub interval: Duration,
    /// Whether the schedule should create a job immediately.
    pub immediate: bool,
    /// The policy for missed jobs.
    pub missed_policy: ScheduleMissedTimePolicy,
}

/// Scheduling policy based on a cron expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingPolicyCron {
    /// The cron expression.
    pub cron_expression: String,
    /// Whether the schedule should create a job immediately.
    pub immediate: bool,
    /// The policy for missed jobs.
    pub missed_policy: ScheduleMissedTimePolicy,
}

/// Policy for missed jobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleMissedTimePolicy {
    /// Skip any missed times.
    Skip,
    /// Create a job for each missed time.
    Create,
}

/// Policy for new jobs created by a schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleJobCreationPolicy {
    /// Create a new job from the given job definition.
    JobDefinition(ScheduleNewJobDefinition),
}

/// A job definition for a new job created by a schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// A schedule that was cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelledSchedule {
    /// The unique identifier of the schedule.
    pub id: Uuid,
}

/// A pending schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSchedule {
    /// The unique identifier of the schedule.
    pub id: Uuid,
    /// Scheduling policy for the schedule.
    pub job_timing_policy: ScheduleJobTimingPolicy,
    /// Policy for new jobs created by the schedule.
    pub job_creation_policy: ScheduleJobCreationPolicy,
    /// The target execution time of the last
    /// job created by the schedule, if any.
    pub last_target_execution_time: Option<SystemTime>,
    /// The time range for the schedule.
    pub time_range: Option<ScheduleTimeRange>,
}

/// Filters for querying schedules.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScheduleQueryFilters {
    /// Schedule IDs to filter the query results by.
    pub schedule_ids: Option<IndexSet<Uuid>>,
    /// Job IDs to filter the query results by.
    pub job_ids: Option<IndexSet<Uuid>>,
    /// Job type IDs to filter the query results by.
    pub job_type_ids: Option<IndexSet<String>>,
    /// Labels to filter the query results by.
    pub labels: Option<IndexMap<String, ScheduleLabelFilterValue>>,
    /// Whether to return active or inactive schedules only.
    pub active: Option<bool>,
    /// Schedules created after the given time, inclusive.
    pub created_after: Option<SystemTime>,
    /// Schedules created before the given time, exclusive.
    pub created_before: Option<SystemTime>,
}

/// The order of jobs returned.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ScheduleQueryOrder {
    /// Order by the time the job was created in ascending order.
    CreatedAtAsc,
    /// Order by the time the job was created in descending order.
    CreatedAtDesc,
}

/// The results of a schedule query.
#[derive(Debug, Clone)]
pub struct ScheduleQueryResult {
    /// The schedules that satisfy the query.
    pub schedules: Vec<ScheduleDetails>,
    /// A cursor that can be used to continue the query.
    pub cursor: Option<String>,
    /// Whether there are more results to query.
    pub has_more: bool,
}

/// Details of a schedule.
///
/// Associated jobs are not included on purpose
/// as there can be many jobs associated with a schedule,
/// additional queries can be made to get the jobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleDetails {
    /// The unique identifier of the schedule.
    pub id: Uuid,
    /// The time the schedule was created.
    pub created_at: SystemTime,
    /// Scheduling policy for the schedule.
    pub job_timing_policy: ScheduleJobTimingPolicy,
    /// Policy for new jobs created by the schedule.
    pub job_creation_policy: ScheduleJobCreationPolicy,
    /// Labels of the schedule.
    pub labels: IndexMap<String, String>,
    /// Whether the schedule is active.
    pub active: bool,
    /// Whether the schedule was cancelled.
    pub cancelled: bool,
    /// The time range for the schedule.
    pub time_range: Option<ScheduleTimeRange>,
    /// Arbitrary metadata in JSON format.
    pub metadata_json: Option<String>,
}

/// Schedule label filter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScheduleLabelFilterValue {
    /// The label exists with any value.
    Exists,
    /// The label does not exist.
    Equals(String),
}

/// The time range for a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleTimeRange {
    /// The schedule must not start before this time.
    pub start: Option<SystemTime>,
    /// The schedule must end before this time.
    pub end: Option<SystemTime>,
}

impl ScheduleTimeRange {
    /// Check if the given time is within the time range.
    #[must_use]
    pub fn contains(&self, time: SystemTime) -> bool {
        if let Some(start) = self.start {
            if time < start {
                return false;
            }
        }
        if let Some(end) = self.end {
            if time >= end {
                return false;
            }
        }
        true
    }

    /// Check if the time range is valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if let (Some(start), Some(end)) = (self.start, self.end) {
            start < end
        } else {
            true
        }
    }
}

impl From<ScheduleJobTimingPolicy> for common::v1::ScheduleJobTimingPolicy {
    fn from(value: ScheduleJobTimingPolicy) -> Self {
        match value {
            ScheduleJobTimingPolicy::Repeat(policy) => common::v1::ScheduleJobTimingPolicy {
                job_timing: Some(schedule_job_timing_policy::JobTiming::Repeat(
                    common::v1::ScheduleJobTimingPolicyRepeat {
                        interval: Some(policy.interval.try_into().unwrap()),
                        immediate: policy.immediate,
                        missed_time_policy: common::v1::ScheduleMissedTimePolicy::from(
                            policy.missed_policy,
                        )
                        .into(),
                    },
                )),
            },
            ScheduleJobTimingPolicy::Cron(policy) => common::v1::ScheduleJobTimingPolicy {
                job_timing: Some(schedule_job_timing_policy::JobTiming::Cron(
                    common::v1::ScheduleJobTimingPolicyCron {
                        cron_expression: policy.cron_expression,
                        immediate: policy.immediate,
                        missed_time_policy: common::v1::ScheduleMissedTimePolicy::from(
                            policy.missed_policy,
                        )
                        .into(),
                    },
                )),
            },
        }
    }
}

impl TryFrom<common::v1::ScheduleJobTimingPolicy> for ScheduleJobTimingPolicy {
    type Error = Status;

    fn try_from(value: common::v1::ScheduleJobTimingPolicy) -> Result<Self, Self::Error> {
        let value = value.job_timing.ok_or_else(|| {
            Status::invalid_argument("missing job timing policy in schedule definition")
        })?;

        match value {
            schedule_job_timing_policy::JobTiming::Repeat(policy) => {
                Ok(Self::Repeat(SchedulingPolicyRepeat {
                    interval: policy
                        .interval
                        .ok_or_else(|| {
                            Status::invalid_argument("missing interval in repeat policy")
                        })?
                        .try_into()
                        .map_err(|_| Status::invalid_argument("invalid interval"))?,
                    immediate: policy.immediate,
                    missed_policy: policy.missed_time_policy().into(),
                }))
            }
            schedule_job_timing_policy::JobTiming::Cron(policy) => {
                Ok(Self::Cron(SchedulingPolicyCron {
                    missed_policy: policy.missed_time_policy().into(),
                    cron_expression: {
                        let mut parse_options = cronexpr::ParseOptions::default();
                        parse_options.fallback_timezone_option =
                            cronexpr::FallbackTimezoneOption::UTC;

                        // Validate the cron expression.
                        cronexpr::parse_crontab_with(&policy.cron_expression, parse_options)
                            .map_err(|err| {
                                Status::invalid_argument(format!("invalid cron expression: {err}"))
                            })?;

                        policy.cron_expression
                    },
                    immediate: policy.immediate,
                }))
            }
        }
    }
}

impl From<ScheduleJobCreationPolicy> for common::v1::ScheduleJobCreationPolicy {
    fn from(value: ScheduleJobCreationPolicy) -> Self {
        match value {
            ScheduleJobCreationPolicy::JobDefinition(job_definition) => {
                common::v1::ScheduleJobCreationPolicy {
                    job_creation: Some(JobCreation::JobDefinition(common::v1::JobDefinition {
                        job_type_id: job_definition.job_type_id,
                        input_payload_json: job_definition.input_payload_json,
                        target_execution_time: None,
                        retry_policy: Some(common::v1::JobRetryPolicy {
                            retries: job_definition.retry_policy.retries,
                        }),
                        timeout_policy: Some(common::v1::JobTimeoutPolicy {
                            timeout: job_definition
                                .timeout_policy
                                .timeout
                                .and_then(|d| d.try_into().ok()),
                            base_time: match job_definition.timeout_policy.base_time {
                                JobTimeoutBaseTime::StartTime => {
                                    common::v1::JobTimeoutBaseTime::StartTime.into()
                                }
                                JobTimeoutBaseTime::TargetExecutionTime => {
                                    common::v1::JobTimeoutBaseTime::TargetExecutionTime.into()
                                }
                            },
                        }),
                        labels: job_definition
                            .labels
                            .into_iter()
                            .map(|(key, value)| common::v1::JobLabel { key, value })
                            .collect(),
                        metadata_json: None,
                    })),
                }
            }
        }
    }
}

impl TryFrom<common::v1::ScheduleJobCreationPolicy> for ScheduleJobCreationPolicy {
    type Error = Status;

    fn try_from(value: common::v1::ScheduleJobCreationPolicy) -> Result<Self, Self::Error> {
        let value = value.job_creation.ok_or_else(|| {
            Status::invalid_argument("missing job creation policy in schedule definition")
        })?;

        match value {
            JobCreation::JobDefinition(job_definition) => {
                Ok(Self::JobDefinition(ScheduleNewJobDefinition {
                    job_type_id: job_definition.job_type_id,
                    input_payload_json: job_definition.input_payload_json,
                    timeout_policy: job_definition
                        .timeout_policy
                        .map(Into::into)
                        .unwrap_or_default(),
                    retry_policy: job_definition
                        .retry_policy
                        .map(Into::into)
                        .unwrap_or_default(),
                    labels: job_definition
                        .labels
                        .into_iter()
                        .map(|label| (label.key, label.value))
                        .collect(),
                }))
            }
        }
    }
}

impl From<ScheduleTimeRange> for TimeRange {
    fn from(value: ScheduleTimeRange) -> Self {
        Self {
            start: value.start.map(Into::into),
            end: value.end.map(Into::into),
        }
    }
}

impl TryFrom<TimeRange> for ScheduleTimeRange {
    type Error = Status;

    fn try_from(value: TimeRange) -> Result<Self, Self::Error> {
        Ok(Self {
            start: value
                .start
                .map(SystemTime::try_from)
                .transpose()
                .map_err(|error| {
                    Status::invalid_argument(format!("unsupported timestamp: {error}"))
                })?,
            end: value
                .end
                .map(SystemTime::try_from)
                .transpose()
                .map_err(|error| {
                    Status::invalid_argument(format!("unsupported timestamp: {error}"))
                })?,
        })
    }
}

impl From<common::v1::ScheduleMissedTimePolicy> for ScheduleMissedTimePolicy {
    fn from(value: common::v1::ScheduleMissedTimePolicy) -> Self {
        match value {
            common::v1::ScheduleMissedTimePolicy::Skip
            | common::v1::ScheduleMissedTimePolicy::Unspecified => Self::Skip,
            common::v1::ScheduleMissedTimePolicy::Create => Self::Create,
        }
    }
}

impl From<ScheduleMissedTimePolicy> for common::v1::ScheduleMissedTimePolicy {
    fn from(value: ScheduleMissedTimePolicy) -> Self {
        match value {
            ScheduleMissedTimePolicy::Skip => Self::Skip,
            ScheduleMissedTimePolicy::Create => Self::Create,
        }
    }
}

impl From<server::v1::JobQueryOrder> for JobQueryOrder {
    fn from(value: server::v1::JobQueryOrder) -> Self {
        match value {
            server::v1::JobQueryOrder::CreatedAtAsc => Self::CreatedAtAsc,
            server::v1::JobQueryOrder::CreatedAtDesc | server::v1::JobQueryOrder::Unspecified => {
                Self::CreatedAtDesc
            }
            server::v1::JobQueryOrder::TargetExecutionTimeAsc => Self::TargetExecutionTimeAsc,
            server::v1::JobQueryOrder::TargetExecutionTimeDesc => Self::TargetExecutionTimeDesc,
        }
    }
}

impl From<JobDetails> for Job {
    fn from(job: JobDetails) -> Self {
        Job {
            id: job.id.to_string(),
            schedule_id: job.schedule_id.map(|t| t.to_string()),
            cancelled: job.cancelled,
            active: job.active,
            definition: Some(JobDefinition {
                job_type_id: job.job_type_id,
                input_payload_json: job.input_payload_json,
                target_execution_time: Some(job.target_execution_time.into()),
                retry_policy: Some(common::v1::JobRetryPolicy {
                    retries: job.retry_policy.retries,
                }),
                timeout_policy: Some(common::v1::JobTimeoutPolicy {
                    timeout: job.timeout_policy.timeout.and_then(|d| d.try_into().ok()),
                    base_time: match job.timeout_policy.base_time {
                        JobTimeoutBaseTime::StartTime => {
                            common::v1::JobTimeoutBaseTime::StartTime.into()
                        }
                        JobTimeoutBaseTime::TargetExecutionTime => {
                            common::v1::JobTimeoutBaseTime::TargetExecutionTime.into()
                        }
                    },
                }),
                labels: job
                    .labels
                    .into_iter()
                    .map(|(key, value)| common::v1::JobLabel { key, value })
                    .collect(),
                metadata_json: job.metadata_json,
            }),
            created_at: Some(job.created_at.into()),
            executions: job.executions.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ExecutionDetails> for server::v1::JobExecution {
    fn from(value: ExecutionDetails) -> Self {
        Self {
            id: value.id.to_string(),
            job_id: value.job_id.to_string(),
            executor_id: value.executor_id.map(|t| t.to_string()),
            status: match value.status {
                JobExecutionStatus::Pending => server::v1::JobExecutionStatus::Pending,
                JobExecutionStatus::Ready => server::v1::JobExecutionStatus::Ready,
                JobExecutionStatus::Assigned => server::v1::JobExecutionStatus::Assigned,
                JobExecutionStatus::Running => server::v1::JobExecutionStatus::Running,
                JobExecutionStatus::Succeeded => server::v1::JobExecutionStatus::Succeeded,
                JobExecutionStatus::Failed => server::v1::JobExecutionStatus::Failed,
            }
            .into(),
            created_at: Some(value.created_at.into()),
            ready_at: value.ready_at.map(Into::into),
            assigned_at: value.assigned_at.map(Into::into),
            started_at: value.started_at.map(Into::into),
            succeeded_at: value.succeeded_at.map(Into::into),
            failed_at: value.failed_at.map(Into::into),
            output_payload_json: value.output_payload_json,
            failure_reason: value.failure_reason,
        }
    }
}

impl From<JobType> for ora_proto::common::v1::JobType {
    fn from(value: JobType) -> Self {
        Self {
            id: value.id,
            name: if value.name.is_empty() {
                None
            } else {
                Some(value.name)
            },
            description: if value.description.is_empty() {
                None
            } else {
                Some(value.description)
            },
            input_schema_json: value.input_schema_json,
            output_schema_json: value.output_schema_json,
        }
    }
}

impl TryFrom<server::v1::JobQueryFilter> for JobQueryFilters {
    type Error = Status;

    fn try_from(filter: server::v1::JobQueryFilter) -> Result<Self, Self::Error> {
        Ok(Self {
            execution_status: {
                let status = filter
                    .status()
                    .filter_map(|status| match status {
                        server::v1::JobExecutionStatus::Unspecified => None,
                        server::v1::JobExecutionStatus::Pending => {
                            Some(JobExecutionStatus::Pending)
                        }
                        server::v1::JobExecutionStatus::Ready => Some(JobExecutionStatus::Ready),
                        server::v1::JobExecutionStatus::Assigned => {
                            Some(JobExecutionStatus::Assigned)
                        }
                        server::v1::JobExecutionStatus::Running => {
                            Some(JobExecutionStatus::Running)
                        }
                        server::v1::JobExecutionStatus::Succeeded => {
                            Some(JobExecutionStatus::Succeeded)
                        }
                        server::v1::JobExecutionStatus::Failed => Some(JobExecutionStatus::Failed),
                    })
                    .collect::<IndexSet<_>>();

                if status.is_empty() {
                    None
                } else {
                    Some(status)
                }
            },
            job_ids: if filter.job_ids.is_empty() {
                None
            } else {
                Some(
                    filter
                        .job_ids
                        .iter()
                        .map(|f| {
                            f.parse().map_err(|err| {
                                Status::invalid_argument(format!("invalid job ID: {err}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            job_type_ids: if filter.job_type_ids.is_empty() {
                None
            } else {
                Some(filter.job_type_ids.into_iter().collect())
            },
            execution_ids: if filter.execution_ids.is_empty() {
                None
            } else {
                Some(
                    filter
                        .execution_ids
                        .iter()
                        .map(|f| {
                            f.parse().map_err(|err| {
                                Status::invalid_argument(format!("invalid execution ID: {err}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            schedule_ids: if filter.schedule_ids.is_empty() {
                None
            } else {
                Some(
                    filter
                        .schedule_ids
                        .iter()
                        .map(|f| {
                            f.parse().map_err(|err| {
                                Status::invalid_argument(format!("invalid schedule ID: {err}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            labels: if filter.labels.is_empty() {
                None
            } else {
                Some(
                    filter
                        .labels
                        .into_iter()
                        .filter_map(|label| {
                            Some((
                                label.key,
                                match label.value? {
                                    server::v1::job_label_filter::Value::Exists(_) => {
                                        JobLabelFilterValue::Exists
                                    }
                                    server::v1::job_label_filter::Value::Equals(value) => {
                                        JobLabelFilterValue::Equals(value)
                                    }
                                },
                            ))
                        })
                        .collect(),
                )
            },
            active: filter.active,
            created_after: filter
                .created_at
                .and_then(|c| c.start.and_then(|t| t.try_into().ok())),
            created_before: filter
                .created_at
                .and_then(|c| c.end.and_then(|t| t.try_into().ok())),
            target_execution_time_after: filter
                .target_execution_time
                .and_then(|c| c.start.and_then(|t| t.try_into().ok())),
            target_execution_time_before: filter
                .target_execution_time
                .and_then(|c| c.end.and_then(|t| t.try_into().ok())),
        })
    }
}

impl From<ScheduleDetails> for server::v1::Schedule {
    fn from(value: ScheduleDetails) -> Self {
        Self {
            id: value.id.to_string(),
            definition: Some(common::v1::ScheduleDefinition {
                job_timing_policy: Some(match value.job_timing_policy {
                    ScheduleJobTimingPolicy::Repeat(policy) => {
                        common::v1::ScheduleJobTimingPolicy {
                            job_timing: Some(schedule_job_timing_policy::JobTiming::Repeat(
                                common::v1::ScheduleJobTimingPolicyRepeat {
                                    interval: policy.interval.try_into().ok(),
                                    immediate: policy.immediate,
                                    missed_time_policy: common::v1::ScheduleMissedTimePolicy::from(
                                        policy.missed_policy,
                                    )
                                    .into(),
                                },
                            )),
                        }
                    }
                    ScheduleJobTimingPolicy::Cron(policy) => common::v1::ScheduleJobTimingPolicy {
                        job_timing: Some(schedule_job_timing_policy::JobTiming::Cron(
                            common::v1::ScheduleJobTimingPolicyCron {
                                cron_expression: policy.cron_expression,
                                immediate: policy.immediate,
                                missed_time_policy: common::v1::ScheduleMissedTimePolicy::from(
                                    policy.missed_policy,
                                )
                                .into(),
                            },
                        )),
                    },
                }),
                job_creation_policy: Some(common::v1::ScheduleJobCreationPolicy {
                    job_creation: Some(match value.job_creation_policy {
                        ScheduleJobCreationPolicy::JobDefinition(job_definition) => {
                            JobCreation::JobDefinition(common::v1::JobDefinition {
                                job_type_id: job_definition.job_type_id,
                                input_payload_json: job_definition.input_payload_json,
                                target_execution_time: None,
                                retry_policy: Some(common::v1::JobRetryPolicy {
                                    retries: job_definition.retry_policy.retries,
                                }),
                                timeout_policy: Some(common::v1::JobTimeoutPolicy {
                                    timeout: job_definition
                                        .timeout_policy
                                        .timeout
                                        .and_then(|d| d.try_into().ok()),
                                    base_time: match job_definition.timeout_policy.base_time {
                                        JobTimeoutBaseTime::StartTime => {
                                            common::v1::JobTimeoutBaseTime::StartTime.into()
                                        }
                                        JobTimeoutBaseTime::TargetExecutionTime => {
                                            common::v1::JobTimeoutBaseTime::TargetExecutionTime
                                                .into()
                                        }
                                    },
                                }),
                                labels: job_definition
                                    .labels
                                    .into_iter()
                                    .map(|(key, value)| common::v1::JobLabel { key, value })
                                    .collect(),
                                metadata_json: None,
                            })
                        }
                    }),
                }),
                time_range: value.time_range.map(|range| TimeRange {
                    start: range.start.map(Into::into),
                    end: range.end.map(Into::into),
                }),
                labels: value
                    .labels
                    .into_iter()
                    .map(|(key, value)| common::v1::ScheduleLabel { key, value })
                    .collect(),
                metadata_json: value.metadata_json,
            }),
            created_at: Some(value.created_at.into()),
            active: value.active,
            cancelled: value.cancelled,
        }
    }
}

impl From<server::v1::ScheduleQueryOrder> for ScheduleQueryOrder {
    fn from(value: server::v1::ScheduleQueryOrder) -> Self {
        match value {
            server::v1::ScheduleQueryOrder::CreatedAtAsc => Self::CreatedAtAsc,
            server::v1::ScheduleQueryOrder::CreatedAtDesc
            | server::v1::ScheduleQueryOrder::Unspecified => Self::CreatedAtDesc,
        }
    }
}

impl TryFrom<server::v1::ScheduleQueryFilter> for ScheduleQueryFilters {
    type Error = Status;

    fn try_from(value: server::v1::ScheduleQueryFilter) -> Result<Self, Self::Error> {
        Ok(Self {
            schedule_ids: if value.schedule_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .schedule_ids
                        .iter()
                        .map(|f| {
                            f.parse().map_err(|err| {
                                Status::invalid_argument(format!("invalid schedule ID: {err}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            job_ids: if value.job_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .job_ids
                        .iter()
                        .map(|f| {
                            f.parse().map_err(|err| {
                                Status::invalid_argument(format!("invalid job ID: {err}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            job_type_ids: if value.job_type_ids.is_empty() {
                None
            } else {
                Some(value.job_type_ids.into_iter().collect())
            },
            labels: if value.labels.is_empty() {
                None
            } else {
                Some(
                    value
                        .labels
                        .into_iter()
                        .filter_map(|label| {
                            Some((
                                label.key,
                                match label.value? {
                                    server::v1::schedule_label_filter::Value::Exists(_) => {
                                        ScheduleLabelFilterValue::Exists
                                    }
                                    server::v1::schedule_label_filter::Value::Equals(value) => {
                                        ScheduleLabelFilterValue::Equals(value)
                                    }
                                },
                            ))
                        })
                        .collect(),
                )
            },
            active: value.active,
            created_after: value
                .created_at
                .and_then(|c| c.start.and_then(|t| t.try_into().ok())),
            created_before: value
                .created_at
                .and_then(|c| c.end.and_then(|t| t.try_into().ok())),
        })
    }
}

impl From<JobType> for snapshot::v1::ExportedJobType {
    fn from(job_type: JobType) -> Self {
        snapshot::v1::ExportedJobType {
            job_type: Some(common::v1::JobType {
                id: job_type.id,
                name: Some(job_type.name),
                description: Some(job_type.description),
                input_schema_json: job_type.input_schema_json,
                output_schema_json: job_type.output_schema_json,
            }),
        }
    }
}

impl From<snapshot::v1::ExportedJobType> for JobType {
    fn from(job_type: snapshot::v1::ExportedJobType) -> Self {
        let job_type = job_type.job_type.unwrap();

        Self {
            id: job_type.id,
            name: job_type.name.unwrap_or_default(),
            description: job_type.description.unwrap_or_default(),
            input_schema_json: job_type.input_schema_json,
            output_schema_json: job_type.output_schema_json,
        }
    }
}
