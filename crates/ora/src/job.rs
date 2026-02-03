//! Jobs and related types and utilities.

use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    job_type::JobType,
    schedule::{MissedTimePolicy, ScheduleDefinition, SchedulingPolicy},
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

/// A job definition of the given job type.
#[must_use]
#[derive(Debug)]
pub struct JobDefinition<J> {
    /// The target execution time of the job.
    pub target_execution_time: SystemTime,
    /// The input of the job,
    pub input: J,
    /// Labels associated with the job.
    pub labels: BTreeMap<String, String>,
    /// The timeout policy of the job.
    pub timeout_policy: TimeoutPolicy,
    /// The retry policy of the job.
    pub retry_policy: RetryPolicy,
}

impl<J> JobDefinition<J> {
    /// Create a new job.
    pub fn new(target_execution_time: SystemTime, input: J) -> Self {
        Self {
            target_execution_time,
            input,
            labels: BTreeMap::new(),
            timeout_policy: TimeoutPolicy::default(),
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Add a label to the job.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Set the timeout policy of the job.
    ///
    /// Setting a zero timeout duration means the job has no timeout.
    pub fn with_timeout(mut self, timeout: Duration, base_time: TimeoutBaseTime) -> Self {
        self.timeout_policy = TimeoutPolicy { timeout, base_time };
        self
    }

    /// Set the the retry count of the job.
    ///
    /// Setting retries to zero means the job will not be retried.
    pub fn with_retries(mut self, retries: u64) -> Self {
        self.retry_policy.retries = retries;
        self
    }
}

impl<J> JobDefinition<J> {
    /// Use this job definition as a template for a schedule that
    /// repeats at a fixed interval.
    pub fn schedule_interval(self, interval: Duration) -> ScheduleDefinition<J> {
        ScheduleDefinition {
            scheduling: SchedulingPolicy::FixedInterval {
                interval,
                immediate: false,
                missed: MissedTimePolicy::default(),
            },
            job_template: self,
            labels: BTreeMap::new(),
            time_range: Default::default(),
        }
    }

    /// Use this job definition as a template for a schedule that
    /// repeats according to a cron expression.
    ///
    /// # Errors
    ///
    /// This function will return an error if the cron expression is invalid.
    pub fn schedule_cron(
        self,
        expression: impl Into<String>,
    ) -> crate::Result<ScheduleDefinition<J>> {
        let expression = expression.into();

        let mut cron_parse_options = cronexpr::ParseOptions::default();
        cron_parse_options.fallback_timezone_option = cronexpr::FallbackTimezoneOption::System;

        _ = cronexpr::parse_crontab_with(&expression, cron_parse_options)
            .map_err(crate::Error::InvalidCronExpression)?;

        Ok(ScheduleDefinition {
            scheduling: SchedulingPolicy::Cron {
                expression,
                immediate: false,
                missed: MissedTimePolicy::default(),
            },
            job_template: self,
            labels: BTreeMap::new(),
            time_range: Default::default(),
        })
    }
}

/// Timeout policy for a job.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TimeoutPolicy {
    /// The timeout duration.
    ///
    /// If zero, the job has no timeout.
    pub timeout: Duration,
    /// The base time of the timeout.
    pub base_time: TimeoutBaseTime,
}

/// The base time of the timeout.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub enum TimeoutBaseTime {
    /// The timeout is measured from the job's target execution time.
    TargetExecutionTime,
    /// The timeout is measured from the actual start time of the job.
    #[default]
    StartTime,
}

/// Retry policy for a job.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// The maximum number of retries.
    ///
    /// If zero, the job will not be retried.
    pub retries: u64,
}

/// Helper trait implemented for types implementing [`JobType`]
pub trait IntoJob: private::Sealed + Sized {
    /// Create a job with execution time set to the current time.
    fn now(self) -> JobDefinition<Self>;

    /// Create a job with the specified target execution time.
    fn at(self, target_execution_time: SystemTime) -> JobDefinition<Self>;

    /// Create a schedule definition that repeats at a fixed interval.
    fn schedule_interval(self, interval: Duration) -> ScheduleDefinition<Self>;

    /// Create a schedule definition that repeats according to a cron expression.
    ///
    /// # Errors
    ///
    /// This function will return an error if the cron expression is invalid.
    fn schedule_cron(
        self,
        expression: impl Into<String>,
    ) -> crate::Result<ScheduleDefinition<Self>>;
}

impl<T> IntoJob for T
where
    T: JobType + private::Sealed,
{
    fn now(self) -> JobDefinition<Self> {
        JobDefinition::new(SystemTime::now(), self)
    }

    fn at(self, target_execution_time: SystemTime) -> JobDefinition<Self> {
        JobDefinition::new(target_execution_time, self)
    }

    fn schedule_interval(self, interval: Duration) -> ScheduleDefinition<Self> {
        self.now().schedule_interval(interval)
    }

    fn schedule_cron(
        self,
        expression: impl Into<String>,
    ) -> crate::Result<ScheduleDefinition<Self>> {
        self.now().schedule_cron(expression)
    }
}

mod private {
    use crate::job_type::JobType;

    pub trait Sealed {}
    impl<T> Sealed for T where T: JobType {}
}
