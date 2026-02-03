//! Schedules and related types.

use std::{collections::BTreeMap, time::Duration};

use uuid::Uuid;

use crate::{
    common::TimeRange,
    job::{JobDefinition, TimeoutBaseTime},
};

/// A schedule ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Definition of a schedule of a given job.
#[derive(Debug)]
#[must_use]
pub struct ScheduleDefinition<J> {
    /// The scheduling policy for the schedule.
    pub scheduling: SchedulingPolicy,
    /// The job template to be used when creating new jobs.
    pub job_template: JobDefinition<J>,
    /// The labels of the schedule.
    pub labels: BTreeMap<String, String>,
    /// The time range for the schedule.
    pub time_range: TimeRange,
}

impl<J> ScheduleDefinition<J> {
    /// Add a label to the schedule.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Set the schedule to create a job immediately upon creation.
    pub fn immediate(mut self) -> Self {
        match &mut self.scheduling {
            SchedulingPolicy::FixedInterval { immediate, .. }
            | SchedulingPolicy::Cron { immediate, .. } => {
                *immediate = true;
            }
        }
        self
    }

    /// Set the timeout policy of the jobs created by this schedule.
    ///
    /// Setting a zero timeout duration means the job has no timeout.
    pub fn with_timeout(mut self, timeout: Duration, base_time: TimeoutBaseTime) -> Self {
        self.job_template = self.job_template.with_timeout(timeout, base_time);
        self
    }

    /// Set the the retry count of the jobs created by this schedule.
    ///
    /// Setting retries to zero means the job will not be retried.
    pub fn with_retries(mut self, retries: u64) -> Self {
        self.job_template = self.job_template.with_retries(retries);
        self
    }
}

/// The scheduling policy of a schedule.
#[derive(Debug)]
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MissedTimePolicy {
    /// Skip missed execution times.
    #[default]
    Skip,
    /// Create jobs for missed execution times.
    Create,
}
