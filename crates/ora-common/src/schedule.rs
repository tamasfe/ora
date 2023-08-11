//! Schedules for repeated task executions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::Duration;

use crate::task::TaskDefinition;

/// A schedule supports repeatedly spawning jobs
/// based on the given settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct ScheduleDefinition {
    /// The task schedule policy.
    pub policy: SchedulePolicy,
    /// Whether to immediately spawn a task
    /// when the schedule is first processed.
    pub immediate: bool,
    /// The policy for missed tasks.
    #[serde(default)]
    pub missed_tasks: MissedTasksPolicy,
    /// Parameters for newly spawned tasks.
    pub new_task: NewTask,
    /// Schedule labels.
    #[serde(default)]
    pub labels: HashMap<String, Value>,
}

impl ScheduleDefinition {
    /// Create a new schedule.
    pub fn new(policy: SchedulePolicy, new_task: NewTask) -> Self {
        Self {
            missed_tasks: Default::default(),
            policy,
            new_task,
            immediate: false,
            labels: Default::default(),
        }
    }

    /// Set whether a task should be immediately scheduled
    /// when the schedule is added.
    pub fn immediate(mut self, immediate: bool) -> Self {
        self.immediate = immediate;
        self
    }

    /// Set the missed tasks policy.
    pub fn on_missed_tasks(mut self, policy: MissedTasksPolicy) -> Self {
        self.missed_tasks = policy;
        self
    }

    /// Set a label value.
    ///
    /// # Panics
    ///
    /// Panics if the value is not JSON-serializable.
    pub fn with_label(mut self, name: &str, value: impl Serialize) -> Self {
        self.labels
            .insert(name.into(), serde_json::to_value(value).unwrap());
        self
    }
}

/// Task spawning policy of the schedule.
///
/// It is used to configure how and when new
/// tasks are spawned.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[must_use]
pub enum SchedulePolicy {
    /// Repeatedly spawn tasks.
    Repeat {
        /// The interval between tasks.
        interval_ns: u64,
    },
    /// Repeat using a cron expression.
    Cron {
        /// A cron expression.
        expr: String,
    },
}

impl SchedulePolicy {
    /// Repeat tasks with the given interval.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn repeat(interval: Duration) -> Self {
        Self::Repeat {
            interval_ns: interval.whole_nanoseconds() as _,
        }
    }

    /// Repeat tasks according to the given cron expression.
    ///
    /// # Errors
    ///
    /// Returns an error if the cron expression is not valid.
    pub fn cron(expr: &str) -> Result<Self, cron::error::Error> {
        expr.parse::<cron::Schedule>()?;

        Ok(Self::Cron {
            expr: expr.to_string(),
        })
    }
}

/// The policy that is used to determine
/// the execution target time of newly spawned
/// tasks when the schedule is behind.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedTasksPolicy {
    /// Queue all missed tasks.
    Burst,
    /// Skip all missed tasks and set
    /// the next task at a multiple of the interval.
    #[default]
    Skip,
}

/// Parameters for newly spawned tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewTask {
    /// Spawn the same task repeatedly.
    Repeat {
        /// The task to be repeated.
        task: TaskDefinition,
    },
}

impl NewTask {
    /// Use the same task for each run.
    #[must_use]
    pub fn repeat<T>(task: TaskDefinition<T>) -> Self {
        Self::Repeat { task: task.cast() }
    }
}
