use std::collections::HashMap;

use async_graphql::{ComplexObject, Enum, InputObject, OneofObject, SimpleObject, Union};
use base64::Engine;
use ora_client::RawTaskResult;
use ora_common::{
    schedule::{MissedTasksPolicy, NewTask, ScheduleDefinition, SchedulePolicy},
    task::{TaskDataFormat, TaskDefinition, TaskStatus, WorkerSelector},
    timeout::TimeoutPolicy,
};
use serde_json::Value;
use time::{Duration, OffsetDateTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Enum)]
#[graphql(name = "TaskStatus")]
pub(crate) enum GqlTaskStatus {
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

impl From<GqlTaskStatus> for TaskStatus {
    fn from(value: GqlTaskStatus) -> Self {
        match value {
            GqlTaskStatus::Pending => Self::Pending,
            GqlTaskStatus::Ready => Self::Ready,
            GqlTaskStatus::Started => Self::Started,
            GqlTaskStatus::Succeeded => Self::Succeeded,
            GqlTaskStatus::Failed => Self::Failed,
            GqlTaskStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<TaskStatus> for GqlTaskStatus {
    fn from(value: TaskStatus) -> Self {
        match value {
            TaskStatus::Pending => Self::Pending,
            TaskStatus::Ready => Self::Ready,
            TaskStatus::Started => Self::Started,
            TaskStatus::Succeeded => Self::Succeeded,
            TaskStatus::Failed => Self::Failed,
            TaskStatus::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, InputObject, SimpleObject)]
#[graphql(complex, input_name = "InputTaskDefinition", name = "TaskDefinition")]
pub(crate) struct GqlTaskDefinition {
    /// The target time of the task execution.
    pub(crate) target: OffsetDateTime,
    /// The worker selector of the task.
    pub(crate) worker_selector: GqlWorkerSelector,
    /// Arbitrary task data that is passed to the workers.
    pub(crate) data_base64: String,
    /// The input data format.
    #[graphql(default)]
    pub(crate) data_format: GqlTaskDataFormat,
    /// Arbitrary task labels.
    #[graphql(default)]
    pub(crate) labels: Vec<Label>,
    /// An optional timeout policy.
    #[graphql(default)]
    pub(crate) timeout: GqlTimeoutPolicy,
}

#[ComplexObject]
impl GqlTaskDefinition {
    /// The data in JSON format, only available if the data format is JSON.
    async fn data_json(&self) -> Option<Value> {
        match self.data_format {
            GqlTaskDataFormat::Json => base64::prelude::BASE64_STANDARD
                .decode(&self.data_base64)
                .ok()
                .and_then(|data| serde_json::from_slice(&data).ok()),
            _ => None,
        }
    }
}

impl From<GqlTaskDefinition> for TaskDefinition {
    fn from(value: GqlTaskDefinition) -> Self {
        TaskDefinition {
            target: value.target.into(),
            worker_selector: value.worker_selector.into(),
            data: base64::prelude::BASE64_STANDARD
                .decode(value.data_base64)
                .unwrap(),
            data_format: value.data_format.into(),
            labels: value
                .labels
                .into_iter()
                .fold(HashMap::new(), |mut labels, label| {
                    labels.insert(label.name, label.value);
                    labels
                }),
            timeout: value.timeout.into(),
            _task_type: std::marker::PhantomData,
        }
    }
}

impl From<TaskDefinition> for GqlTaskDefinition {
    fn from(value: TaskDefinition) -> Self {
        GqlTaskDefinition {
            target: value.target.into(),
            worker_selector: value.worker_selector.into(),
            data_base64: base64::prelude::BASE64_STANDARD.encode(&value.data),
            data_format: value.data_format.into(),
            labels: value
                .labels
                .into_iter()
                .map(|(name, value)| Label { name, value })
                .collect(),
            timeout: value.timeout.into(),
        }
    }
}

#[derive(Debug, Clone, InputObject, SimpleObject)]
#[graphql(input_name = "InputWorkerSelector", name = "WorkerSelector")]
pub(crate) struct GqlWorkerSelector {
    kind: String,
}

impl From<GqlWorkerSelector> for WorkerSelector {
    fn from(value: GqlWorkerSelector) -> Self {
        WorkerSelector {
            kind: value.kind.into(),
        }
    }
}

impl From<WorkerSelector> for GqlWorkerSelector {
    fn from(value: WorkerSelector) -> Self {
        GqlWorkerSelector {
            kind: value.kind.into(),
        }
    }
}

/// The format of the task input or output data.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Enum)]
#[graphql(name = "TaskDataFormat")]
pub(crate) enum GqlTaskDataFormat {
    /// Arbitrary bytes.
    #[default]
    Unknown,
    /// The data can be interpreted as self-describing MessagePack.
    MessagePack,
    /// The data can be interpreted as JSON.
    Json,
}

impl From<GqlTaskDataFormat> for TaskDataFormat {
    fn from(value: GqlTaskDataFormat) -> Self {
        match value {
            GqlTaskDataFormat::Unknown => Self::Unknown,
            GqlTaskDataFormat::MessagePack => Self::MessagePack,
            GqlTaskDataFormat::Json => Self::Json,
        }
    }
}

impl From<TaskDataFormat> for GqlTaskDataFormat {
    fn from(value: TaskDataFormat) -> Self {
        match value {
            TaskDataFormat::Unknown => Self::Unknown,
            TaskDataFormat::MessagePack => Self::MessagePack,
            TaskDataFormat::Json => Self::Json,
        }
    }
}

#[derive(Debug, OneofObject, Union)]
#[graphql(input_name = "InputTimeoutPolicy", name = "TimeoutPolicy")]
pub(crate) enum GqlTimeoutPolicy {
    Never(TimeoutNever),
    FromTarget(TimeoutFromTarget),
}

impl Default for GqlTimeoutPolicy {
    fn default() -> Self {
        GqlTimeoutPolicy::Never(Default::default())
    }
}

#[derive(Debug, Default, InputObject, SimpleObject)]
#[graphql(input_name = "InputTimeoutNever", name = "TimeoutNever")]
pub(crate) struct TimeoutNever {
    pub(crate) timeout: NeverTimeout,
}

#[derive(Debug, InputObject, SimpleObject)]
#[graphql(input_name = "InputTimeoutFromTarget", name = "TimeoutFromTarget")]
pub(crate) struct TimeoutFromTarget {
    pub(crate) ns_from_target: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Enum)]
pub(crate) enum NeverTimeout {
    #[default]
    Never,
}

impl From<GqlTimeoutPolicy> for TimeoutPolicy {
    fn from(value: GqlTimeoutPolicy) -> Self {
        match value {
            GqlTimeoutPolicy::Never(_) => TimeoutPolicy::Never,
            GqlTimeoutPolicy::FromTarget(timeout) => TimeoutPolicy::FromTarget {
                timeout: Duration::nanoseconds(timeout.ns_from_target.try_into().unwrap()),
            },
        }
    }
}

impl From<TimeoutPolicy> for GqlTimeoutPolicy {
    fn from(value: TimeoutPolicy) -> Self {
        match value {
            TimeoutPolicy::Never => GqlTimeoutPolicy::Never(Default::default()),
            TimeoutPolicy::FromTarget { timeout } => {
                GqlTimeoutPolicy::FromTarget(TimeoutFromTarget {
                    ns_from_target: timeout.whole_nanoseconds().try_into().unwrap_or(0),
                })
            }
        }
    }
}

#[derive(Debug, InputObject, SimpleObject)]
#[graphql(input_name = "InputLabel", name = "Label")]
pub(crate) struct Label {
    /// The name of the label.
    pub(crate) name: String,
    /// The value of the label.
    pub(crate) value: Value,
}

#[derive(Debug, InputObject, SimpleObject)]
#[graphql(input_name = "InputScheduleDefinition", name = "ScheduleDefinition")]
pub(crate) struct GqlScheduleDefinition {
    /// The task schedule policy.
    pub(crate) policy: GqlSchedulePolicy,
    /// Whether to immediately spawn a task
    /// when the schedule is first processed.
    pub(crate) immediate: bool,
    /// The policy for missed tasks.
    #[graphql(default)]
    pub(crate) missed_tasks: GqlMissedTasksPolicy,
    /// Parameters for newly spawned tasks.
    pub(crate) new_task: GqlNewTask,
    /// Schedule labels.
    #[graphql(default)]
    pub(crate) labels: Vec<Label>,
}

impl From<GqlScheduleDefinition> for ScheduleDefinition {
    fn from(value: GqlScheduleDefinition) -> Self {
        ScheduleDefinition {
            policy: value.policy.into(),
            immediate: value.immediate,
            missed_tasks: value.missed_tasks.into(),
            new_task: value.new_task.into(),
            labels: value
                .labels
                .into_iter()
                .fold(HashMap::new(), |mut labels, label| {
                    labels.insert(label.name, label.value);
                    labels
                }),
        }
    }
}

impl From<ScheduleDefinition> for GqlScheduleDefinition {
    fn from(value: ScheduleDefinition) -> Self {
        GqlScheduleDefinition {
            policy: value.policy.into(),
            immediate: value.immediate,
            missed_tasks: value.missed_tasks.into(),
            new_task: value.new_task.into(),
            labels: value
                .labels
                .into_iter()
                .map(|(name, value)| Label { name, value })
                .collect(),
        }
    }
}

#[derive(Debug, Union, OneofObject)]
#[graphql(input_name = "InputSchedulePolicy", name = "SchedulePolicy")]
pub(crate) enum GqlSchedulePolicy {
    Repeat(SchedulePolicyRepeat),
    Cron(SchedulePolicyCron),
}

#[derive(Debug, InputObject, SimpleObject)]
#[graphql(
    input_name = "InputSchedulePolicyRepeat",
    name = "SchedulePolicyRepeat"
)]
pub(crate) struct SchedulePolicyRepeat {
    pub(crate) interval_ns: u64,
}

#[derive(Debug, InputObject, SimpleObject)]
#[graphql(input_name = "InputSchedulePolicyCron", name = "SchedulePolicyCron")]
pub(crate) struct SchedulePolicyCron {
    pub(crate) cron: String,
}

impl From<GqlSchedulePolicy> for SchedulePolicy {
    fn from(value: GqlSchedulePolicy) -> Self {
        match value {
            GqlSchedulePolicy::Repeat(SchedulePolicyRepeat { interval_ns }) => {
                Self::Repeat { interval_ns }
            }
            GqlSchedulePolicy::Cron(SchedulePolicyCron { cron }) => Self::Cron { expr: cron },
        }
    }
}

impl From<SchedulePolicy> for GqlSchedulePolicy {
    fn from(value: SchedulePolicy) -> Self {
        match value {
            SchedulePolicy::Repeat { interval_ns } => {
                Self::Repeat(SchedulePolicyRepeat { interval_ns })
            }
            SchedulePolicy::Cron { expr } => Self::Cron(SchedulePolicyCron { cron: expr }),
        }
    }
}

/// The policy that is used to determine
/// the execution target time of newly spawned
/// tasks when the schedule is behind.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Enum)]
#[graphql(name = "MissedTasksPolicy")]
pub(crate) enum GqlMissedTasksPolicy {
    /// Queue all missed tasks.
    Burst,
    /// Skip all missed tasks and set
    /// the next task at a multiple of the interval.
    #[default]
    Skip,
}

impl From<GqlMissedTasksPolicy> for MissedTasksPolicy {
    fn from(value: GqlMissedTasksPolicy) -> Self {
        match value {
            GqlMissedTasksPolicy::Burst => MissedTasksPolicy::Burst,
            GqlMissedTasksPolicy::Skip => MissedTasksPolicy::Skip,
        }
    }
}

impl From<MissedTasksPolicy> for GqlMissedTasksPolicy {
    fn from(value: MissedTasksPolicy) -> Self {
        match value {
            MissedTasksPolicy::Burst => GqlMissedTasksPolicy::Burst,
            MissedTasksPolicy::Skip => GqlMissedTasksPolicy::Skip,
        }
    }
}

#[derive(Debug, Union, OneofObject)]
#[graphql(input_name = "InputNewTask", name = "NewTask")]
pub(crate) enum GqlNewTask {
    Repeat(RepeatTask),
}

#[derive(Debug, InputObject, SimpleObject)]
#[graphql(input_name = "InputRepeatTask", name = "RepeatTask")]
pub(crate) struct RepeatTask {
    repeat_task: GqlTaskDefinition,
}

impl From<GqlNewTask> for NewTask {
    fn from(value: GqlNewTask) -> Self {
        match value {
            GqlNewTask::Repeat(task) => Self::Repeat {
                task: task.repeat_task.into(),
            },
        }
    }
}

impl From<NewTask> for GqlNewTask {
    fn from(value: NewTask) -> Self {
        match value {
            NewTask::Repeat { task } => Self::Repeat(RepeatTask {
                repeat_task: task.into(),
            }),
        }
    }
}

#[derive(Debug, Union)]
#[graphql(name = "TaskResult")]
pub(crate) enum GqlTaskResult {
    Success(TaskResultSuccess),
    Failure(TaskResultFailure),
    Cancellation(TaskResultCancellation),
}

impl From<RawTaskResult> for GqlTaskResult {
    fn from(value: RawTaskResult) -> Self {
        match value {
            RawTaskResult::Success {
                output_format,
                output,
            } => GqlTaskResult::Success(TaskResultSuccess {
                data_format: output_format.into(),
                output: match output_format {
                    TaskDataFormat::Unknown | TaskDataFormat::MessagePack => {
                        TaskSuccessOutput::Bytes(TaskResultDataBytes {
                            data_base64: base64::prelude::BASE64_STANDARD.encode(&output),
                        })
                    }
                    TaskDataFormat::Json => TaskSuccessOutput::Json(TaskResultDataJson {
                        data_json: serde_json::from_slice(&output).unwrap_or_default(),
                    }),
                },
            }),
            RawTaskResult::Failure { reason } => GqlTaskResult::Failure(TaskResultFailure {
                failure_reason: reason,
            }),
            RawTaskResult::Cancelled => GqlTaskResult::Cancellation(TaskResultCancellation {
                cancellation_reason: None,
            }),
        }
    }
}

#[derive(Debug, SimpleObject)]
pub(crate) struct TaskResultSuccess {
    pub(crate) data_format: GqlTaskDataFormat,
    pub(crate) output: TaskSuccessOutput,
}

#[derive(Debug, Union)]
pub(crate) enum TaskSuccessOutput {
    Bytes(TaskResultDataBytes),
    Json(TaskResultDataJson),
}

#[derive(Debug, SimpleObject)]
pub(crate) struct TaskResultDataBytes {
    pub(crate) data_base64: String,
}

#[derive(Debug, SimpleObject)]
pub(crate) struct TaskResultDataJson {
    pub(crate) data_json: Value,
}

#[derive(Debug, SimpleObject)]
pub(crate) struct TaskResultFailure {
    pub(crate) failure_reason: String,
}

#[derive(Debug, SimpleObject)]
pub(crate) struct TaskResultCancellation {
    pub(crate) cancellation_reason: Option<String>,
}
