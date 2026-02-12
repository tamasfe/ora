//! Schedules and related types.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    common::{Label, LabelFilter, TimeRange},
    jobs::{JobDefinition, JobTypeId},
};

/// A schedule ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ScheduleId(pub Uuid);

impl std::fmt::Display for ScheduleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ScheduleId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ScheduleId> for Uuid {
    fn from(value: ScheduleId) -> Self {
        value.0
    }
}

/// Definition of a schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleDefinition {
    /// The scheduling policy for the schedule.
    pub scheduling: SchedulingPolicy,
    /// The job template to be used when creating new jobs.
    pub job_template: JobDefinition,
    /// The labels of the schedule.
    pub labels: Vec<Label>,
    /// The time range for the schedule.
    pub time_range: TimeRange,
}

/// The scheduling policy of a schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchedulingPolicy {
    /// Repeat at fixed intervals.
    FixedInterval {
        /// The interval between executions.
        interval: Duration,
        /// Whether to immediately create
        /// a job upon schedule creation.
        immediate: bool,
        /// Policy for handling missed executions.
        missed: MissedTimePolicy,
    },
    /// Repeat according to a cron expression.
    Cron {
        /// The cron expression defining the schedule.
        expression: String,
        /// Whether to immediately create
        /// a job upon schedule creation.
        immediate: bool,
        /// Policy for handling missed executions.
        missed: MissedTimePolicy,
    },
}

/// Policy for handling missed execution times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissedTimePolicy {
    /// Skip missed execution times.
    Skip,
    /// Create jobs for missed execution times.
    Create,
}

/// Schedule details.
pub struct ScheduleDetails {
    /// The unique ID of the schedule.
    pub id: ScheduleId,
    /// The schedule definition.
    pub schedule: ScheduleDefinition,
    /// The timestamp when the schedule was created.
    pub created_at: SystemTime,
    /// The status of the schedule.
    pub status: ScheduleStatus,
    /// The timestamp when the schedule was stopped (if any).
    pub stopped_at: Option<SystemTime>,
}

/// The status of a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleStatus {
    /// The schedule is active.
    Active,
    /// The schedule was stopped.
    Stopped,
}

/// Filters for listing schedules.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ScheduleFilters {
    /// Filter by schedule IDs.
    pub schedule_ids: Option<Vec<ScheduleId>>,
    /// Filter by job type IDs.
    pub job_type_ids: Option<Vec<JobTypeId>>,
    /// Filter by status.
    pub statuses: Option<Vec<ScheduleStatus>>,
    /// Filter by creation time.
    /// The range can be open-ended in either direction.
    pub created_at: Option<TimeRange>,
    /// Filter by labels.
    pub labels: Option<Vec<LabelFilter>>,
}

/// The ordering options for listing schedules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScheduleOrderBy {
    /// Order by creation time ascending.
    CreatedAtAsc,
    /// Order by creation time descending.
    CreatedAtDesc,
}

/// A schedule that was stopped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoppedSchedule {
    /// The schedule ID.
    pub schedule_id: ScheduleId,
}

/// A schedule that is active but has no active jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSchedule {
    /// The schedule ID.
    pub schedule_id: ScheduleId,
    /// The target execution time of the last job
    /// belonging to this schedule.
    pub last_target_execution_time: Option<SystemTime>,
    /// The scheduling policy.
    pub scheduling: SchedulingPolicy,
    /// The time range of the schedule.
    pub time_range: TimeRange,
    /// The job template.
    pub job_template: JobDefinition,
}

/// The result of adding schedules,
/// indicating whether the schedules were added or already existed.
pub enum AddedSchedules {
    /// The schedules were added successfully.
    Added(Vec<ScheduleId>),
    /// No schedules were added because existing schedules matched the `if_not_exists` filters.
    Existing(Vec<ScheduleId>),
}

impl AddedSchedules {
    /// Returns the IDs of the schedules, regardless of whether they were added or already existed.
    #[must_use]
    pub fn schedule_ids(&self) -> &[ScheduleId] {
        match self {
            AddedSchedules::Added(ids) | AddedSchedules::Existing(ids) => ids,
        }
    }
}
