use std::time::{Duration, SystemTime};

use fjall::Slice;
use ora_storage::IndexMap;
use rkyv::{with::Map, Archive, Deserialize, Serialize};

use uuid::Uuid;

use rkyv::with::AsUnixTime;

use crate::typed::FjallValue;

#[derive(Debug, Archive, Serialize, Deserialize)]
pub(crate) struct JobTypeData {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema_json: Option<String>,
    pub(crate) output_schema_json: Option<String>,
}

impl From<ora_storage::JobType> for JobTypeData {
    fn from(value: ora_storage::JobType) -> Self {
        Self {
            name: value.name,
            description: value.description,
            input_schema_json: value.input_schema_json,
            output_schema_json: value.output_schema_json,
        }
    }
}

impl FjallValue for JobTypeData {
    type View<'a> = &'a ArchivedJobTypeData;

    fn as_slice(&self) -> Slice {
        let v = serialize!(self).unwrap();
        v.into()
    }

    fn view_from_slice(slice: &Slice) -> Self::View<'_> {
        access!(ArchivedJobTypeData, slice).unwrap()
    }
}

#[derive(Debug, Archive, Serialize, Deserialize)]
pub(crate) struct JobData {
    pub(crate) id: Uuid,
    pub(crate) schedule_id: Option<Uuid>,
    #[rkyv(with = AsUnixTime)]
    pub(crate) created_at: SystemTime,
    pub(crate) job_type_id: String,
    #[rkyv(with = AsUnixTime)]
    pub(crate) target_execution_time: SystemTime,
    pub(crate) retry_policy: JobRetryPolicy,
    pub(crate) timeout_policy: JobTimeoutPolicy,
    pub(crate) labels: IndexMap<String, String>,
    pub(crate) input_payload_json: String,
    pub(crate) metadata_json: Option<String>,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) cancelled_at: Option<SystemTime>,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) marked_unschedulable_at: Option<SystemTime>,
}

impl From<ora_storage::NewJob> for JobData {
    fn from(value: ora_storage::NewJob) -> Self {
        Self {
            id: value.id,
            schedule_id: value.schedule_id,
            created_at: value.created_at,
            job_type_id: value.job_type_id,
            target_execution_time: value.target_execution_time,
            retry_policy: value.retry_policy.into(),
            timeout_policy: value.timeout_policy.into(),
            labels: value.labels,
            input_payload_json: value.input_payload_json,
            metadata_json: value.metadata_json,
            cancelled_at: None,
            marked_unschedulable_at: None,
        }
    }
}

impl FjallValue for JobData {
    type View<'a> = &'a ArchivedJobData;

    fn as_slice(&self) -> Slice {
        let v = serialize!(self).unwrap();
        v.into()
    }

    fn view_from_slice(slice: &Slice) -> Self::View<'_> {
        access!(ArchivedJobData, slice).unwrap()
    }
}

#[derive(Debug, Archive, Serialize, Deserialize)]
pub(crate) struct ExecutionData {
    pub(crate) id: Uuid,
    pub(crate) job_id: Uuid,
    pub(crate) executor_id: Option<Uuid>,
    #[rkyv(with = AsUnixTime)]
    pub(crate) created_at: SystemTime,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) ready_at: Option<SystemTime>,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) assigned_at: Option<SystemTime>,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) started_at: Option<SystemTime>,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) succeeded_at: Option<SystemTime>,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) failed_at: Option<SystemTime>,
    pub(crate) output_payload_json: Option<String>,
    pub(crate) failure_reason: Option<String>,

    #[rkyv(with = AsUnixTime)]
    pub(crate) target_execution_time: SystemTime,
}

impl FjallValue for ExecutionData {
    type View<'a> = &'a ArchivedExecutionData;

    fn as_slice(&self) -> Slice {
        let v = serialize!(self).unwrap();
        v.into()
    }

    fn view_from_slice(slice: &Slice) -> Self::View<'_> {
        access!(ArchivedExecutionData, slice).unwrap()
    }
}

#[derive(Debug, Archive, Serialize, Deserialize)]
pub(crate) struct ScheduleData {
    pub(crate) id: Uuid,
    #[rkyv(with = AsUnixTime)]
    pub(crate) created_at: SystemTime,
    pub(crate) job_type_id: Option<String>,
    pub(crate) labels: IndexMap<String, String>,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) marked_unschedulable_at: Option<SystemTime>,
    #[rkyv(with = Map<AsUnixTime>)]
    pub(crate) cancelled_at: Option<SystemTime>,
    pub(crate) job_timing_policy: ScheduleJobTimingPolicy,
    pub(crate) job_creation_policy: ScheduleJobCreationPolicy,
    pub(crate) time_range: Option<ScheduleTimeRange>,
    pub(crate) metadata_json: Option<String>,
}

impl From<ora_storage::NewSchedule> for ScheduleData {
    fn from(schedule: ora_storage::NewSchedule) -> Self {
        Self {
            id: schedule.id,
            created_at: schedule.created_at,
            job_type_id: match &schedule.job_creation_policy {
                ora_storage::ScheduleJobCreationPolicy::JobDefinition(
                    schedule_new_job_definition,
                ) => Some(schedule_new_job_definition.job_type_id.clone()),
            },
            labels: schedule.labels,
            marked_unschedulable_at: None,
            cancelled_at: None,
            job_timing_policy: schedule.job_timing_policy.into(),
            job_creation_policy: schedule.job_creation_policy.into(),
            time_range: schedule.time_range.map(Into::into),
            metadata_json: schedule.metadata_json,
        }
    }
}

impl FjallValue for ScheduleData {
    type View<'a> = &'a ArchivedScheduleData;

    fn as_slice(&self) -> Slice {
        let v = serialize!(self).unwrap();
        v.into()
    }

    fn view_from_slice(slice: &Slice) -> Self::View<'_> {
        access!(ArchivedScheduleData, slice).unwrap()
    }
}

/// The time range for a schedule.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct ScheduleTimeRange {
    /// The schedule must not start before this time.
    #[rkyv(with = Map<AsUnixTime>)]
    pub start: Option<SystemTime>,
    /// The schedule must end before this time.
    #[rkyv(with = Map<AsUnixTime>)]
    pub end: Option<SystemTime>,
}

impl From<ora_storage::ScheduleTimeRange> for ScheduleTimeRange {
    fn from(value: ora_storage::ScheduleTimeRange) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}

impl From<ScheduleTimeRange> for ora_storage::ScheduleTimeRange {
    fn from(value: ScheduleTimeRange) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}

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
