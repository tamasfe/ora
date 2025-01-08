//! Schedule definitions and details.

use std::time::{Duration, SystemTime};

use serde::Serialize;
use uuid::Uuid;

use crate::{IndexMap, JobDefinition};

/// A new schedule.
#[derive(Debug, Clone)]
#[must_use]
pub struct ScheduleDefinition {
    /// Scheduling policy for the schedule.
    pub job_timing_policy: ScheduleJobTimingPolicy,
    /// Policy for new jobs created by the schedule.
    pub job_creation_policy: ScheduleJobCreationPolicy,
    /// Labels of the schedule.
    pub labels: IndexMap<String, String>,
    /// The time range for the schedule.
    ///
    /// The schedule must not start before `start` and must end before `end`.
    pub time_range: Option<ScheduleTimeRange>,
    /// Arbitrary metadata in JSON format.
    pub metadata_json: Option<String>,
    /// Whether to copy schedule labels to created jobs.
    ///
    /// Labels are not overwritten if they already exist on the job.
    ///
    /// Note that this is a client-side operation
    /// that is done before the schedule is submitted to the server
    /// for creation.
    ///
    /// By default, this is `true`.
    pub propagate_labels_to_jobs: bool,
}

impl ScheduleDefinition {
    /// Set the schedule to immediately create a job when started.
    ///
    /// This is a no-op for timing policies that do not support it.
    pub fn immediate(mut self) -> Self {
        match &mut self.job_timing_policy {
            ScheduleJobTimingPolicy::Repeat(policy) => policy.immediate = true,
            ScheduleJobTimingPolicy::Cron(policy) => policy.immediate = true,
        }
        self
    }

    /// Set a start time for the schedule.
    pub fn start_after(mut self, start: SystemTime) -> Self {
        if let Some(time_range) = self.time_range.as_mut() {
            time_range.start = Some(start);
        } else {
            self.time_range = Some(ScheduleTimeRange {
                start: Some(start),
                end: None,
            });
        }
        self
    }

    /// Set an end time for the schedule.
    pub fn end_before(mut self, end: SystemTime) -> Self {
        if let Some(time_range) = self.time_range.as_mut() {
            time_range.end = Some(end);
        } else {
            self.time_range = Some(ScheduleTimeRange {
                start: None,
                end: Some(end),
            });
        }
        self
    }

    /// Add a label to the schedule.
    ///
    /// Note that this will not set any labels
    /// for jobs created by the schedule.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Add a JSON label to the schedule.
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

    /// Set additional metadata for the schedule.
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

    /// Whether to propagate schedule labels to created jobs.
    ///
    /// Labels are not overwritten if they already exist on the job.
    ///
    /// Note that this is a client-side operation
    /// that is done before the schedule is submitted to the server
    /// for creation.
    ///
    /// By default, this is `true`.
    pub fn propagate_labels(mut self, propagate: bool) -> Self {
        self.propagate_labels_to_jobs = propagate;
        self
    }
}

/// Scheduling policy for a schedule.
#[derive(Debug, Clone)]
pub enum ScheduleJobTimingPolicy {
    /// A schedule that repeats.
    Repeat(ScheduleJobTimingPolicyRepeat),
    /// A schedule based on a cron expression.
    Cron(ScheduleJobTimingPolicyCron),
}

/// Scheduling policy for a schedule that repeats.
#[derive(Debug, Clone)]
pub struct ScheduleJobTimingPolicyRepeat {
    /// The interval between each job.
    pub interval: Duration,
    /// Whether the schedule should create a job immediately.
    pub immediate: bool,
    /// The policy for missed jobs.
    pub missed_time_policy: ScheduleMissedTimePolicy,
}

/// Scheduling policy based on a cron expression.
#[derive(Debug, Clone)]
pub struct ScheduleJobTimingPolicyCron {
    /// The cron expression.
    pub cron_expression: String,
    /// Whether the schedule should create a job immediately.
    pub immediate: bool,
    /// The policy for missed jobs.
    pub missed_time_policy: ScheduleMissedTimePolicy,
}

/// Policy for missed jobs.
#[derive(Debug, Default, Clone, Copy)]
pub enum ScheduleMissedTimePolicy {
    /// Skip any missed times.
    #[default]
    Skip,
    /// Create a job for each missed time.
    Create,
}

/// Policy for new jobs created by a schedule.
#[derive(Debug, Clone)]
pub enum ScheduleJobCreationPolicy {
    /// Create a new job from the given job definition.
    JobDefinition(JobDefinition),
}

/// The time range for a schedule.
#[derive(Debug, Clone, Copy)]
pub struct ScheduleTimeRange {
    /// The schedule must not start before this time.
    pub start: Option<SystemTime>,
    /// The schedule must end before this time.
    pub end: Option<SystemTime>,
}

/// Details of a schedule.
///
/// Associated jobs are not included on purpose
/// as there can be many jobs associated with a schedule,
/// additional queries can be made to get the jobs.
#[derive(Debug, Clone)]
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
