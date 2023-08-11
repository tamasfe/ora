//! Task definition and implementations.

use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::Display,
    marker::PhantomData,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Duration, OffsetDateTime};

use crate::{timeout::TimeoutPolicy, UnixNanos};

/// A selector that is used to connect tasks
/// with workers.
/// Currently a task can be executed by a worker only if
/// the worker selector of the task and the worker are equal.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Hash)]
#[must_use]
pub struct WorkerSelector {
    /// A common shared name between the tasks and workers,
    pub kind: Cow<'static, str>,
}

impl<T> From<T> for WorkerSelector
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self {
            kind: value.into().into(),
        }
    }
}

/// An untyped complete task definition
/// that can be added to the queue.
#[derive(Debug, Serialize, Deserialize)]
#[must_use]
pub struct TaskDefinition<T = ()> {
    /// The target time of the task execution.
    pub target: UnixNanos,
    /// The worker selector of the task.
    pub worker_selector: WorkerSelector,
    /// Arbitrary task data that is passed to the workers.
    pub data: Vec<u8>,
    /// The input data format.
    pub data_format: TaskDataFormat,
    /// Arbitrary task labels.
    #[serde(default)]
    pub labels: HashMap<String, Value>,
    /// An optional timeout policy.
    #[serde(default)]
    pub timeout: TimeoutPolicy,
    #[doc(hidden)]
    pub _task_type: PhantomData<T>,
}

impl<T> Clone for TaskDefinition<T> {
    fn clone(&self) -> Self {
        Self {
            target: self.target,
            worker_selector: self.worker_selector.clone(),
            data: self.data.clone(),
            data_format: self.data_format,
            labels: self.labels.clone(),
            timeout: self.timeout,
            _task_type: PhantomData,
        }
    }
}

impl<T> TaskDefinition<T> {
    /// Set a timeout policy.
    pub fn with_timeout(mut self, timeout: impl Into<TimeoutPolicy>) -> Self {
        self.timeout = timeout.into();
        self
    }

    /// Schedule the task immediately.
    pub fn immediate(mut self) -> Self {
        self.target = UnixNanos(0);
        self
    }

    /// Set the target execution time of the new task.
    pub fn at(mut self, target: OffsetDateTime) -> Self {
        let nanos = target.unix_timestamp_nanos();

        self.target = if nanos.is_negative() {
            UnixNanos(0)
        } else {
            UnixNanos(nanos.unsigned_abs().try_into().unwrap_or(u64::MAX))
        };
        self
    }

    /// Schedule the task at the given unix nanosecond duration.
    pub fn at_unix(mut self, target: UnixNanos) -> Self {
        self.target = target;
        self
    }

    /// Schedule the task with the current time as target.
    pub fn now(mut self) -> Self {
        self.target = UnixNanos::now();
        self
    }

    /// Set the target execution time of the new task to
    /// be after the given duration.
    ///
    /// # Panics
    ///
    /// Panics if the system time is before UNIX epoch.
    #[allow(clippy::cast_possible_truncation)]
    pub fn after(mut self, duration: Duration) -> Self {
        let nanos = duration.whole_nanoseconds();
        let nanos = if nanos.is_negative() {
            0
        } else {
            nanos.unsigned_abs().try_into().unwrap_or(u64::MAX)
        };

        self.target = UnixNanos(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .saturating_add(std::time::Duration::from_nanos(nanos))
                .as_nanos() as u64,
        );

        self
    }

    /// Set the worker selector for the given task.
    pub fn with_worker_selector(mut self, selector: impl Into<WorkerSelector>) -> Self {
        self.worker_selector = selector.into();
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

    /// Cast the task to a different task type,
    /// or to erase the task type replacing it with `()`.
    ///
    /// This is not required in most circumstances.
    pub fn cast<U>(self) -> TaskDefinition<U> {
        TaskDefinition {
            target: self.target,
            worker_selector: self.worker_selector,
            data: self.data,
            data_format: self.data_format,
            labels: self.labels,
            timeout: self.timeout,
            _task_type: PhantomData,
        }
    }
}

/// All valid statuses for tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    /// The task is not yet ready to run.
    Pending,
    /// The task is waiting for a worker.
    Ready,
    /// The task is currently running.
    Started,
    /// The task finished successfully.
    Succeeded,
    /// The task failed.
    Failed,
    /// The task was cancelled.
    Cancelled,
}

impl FromStr for TaskStatus {
    type Err = UnexpectedValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "pending" => TaskStatus::Pending,
            "ready" => TaskStatus::Ready,
            "started" => TaskStatus::Started,
            "succeeded" => TaskStatus::Succeeded,
            "failed" => TaskStatus::Failed,
            "cancelled" => TaskStatus::Cancelled,
            _ => Err(UnexpectedValueError(s.to_string()))?,
        })
    }
}

impl TaskStatus {
    /// Return a string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Ready => "ready",
            TaskStatus::Started => "started",
            TaskStatus::Succeeded => "succeeded",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    /// Return whether the status is considered final,
    /// as in there is no possible other status value
    /// to change to under normal circumstances.
    ///
    /// In other words a task is finished if either:
    ///
    /// - it succeeded
    /// - it failed
    /// - it was cancelled
    #[must_use]
    pub fn is_finished(&self) -> bool {
        matches!(
            self,
            TaskStatus::Succeeded | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }
}

/// The format of the task input or output data.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDataFormat {
    /// Arbitrary bytes.
    #[default]
    Unknown,
    /// The data can be interpreted as self-describing MessagePack.
    MessagePack,
    /// The data can be interpreted as JSON.
    Json,
}

impl TaskDataFormat {
    /// Return a string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskDataFormat::Unknown => "unknown",
            TaskDataFormat::MessagePack => "message_pack",
            TaskDataFormat::Json => "json",
        }
    }
}

impl FromStr for TaskDataFormat {
    type Err = UnexpectedValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "unknown" => TaskDataFormat::Unknown,
            "message_pack" => TaskDataFormat::MessagePack,
            "json" => TaskDataFormat::Json,
            _ => Err(UnexpectedValueError(s.to_string()))?,
        })
    }
}

/// Unexpected value received.
#[derive(Debug)]
pub struct UnexpectedValueError(String);

impl Display for UnexpectedValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unexpected value: {}", self.0)
    }
}

impl std::error::Error for UnexpectedValueError {}
