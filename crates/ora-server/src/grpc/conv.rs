use ora_backend::jobs::JobTypeId;
use tonic::Status;
use uuid::Uuid;

use crate::proto;

pub(super) trait ResultErrExt<T>: Sized {
    fn err_status(self) -> Result<T, Status>;
}

impl<T, E> ResultErrExt<T> for Result<T, E>
where
    E: std::error::Error,
{
    fn err_status(self) -> Result<T, Status> {
        self.map_err(backend_err_to_status)
    }
}

fn backend_err_to_status<E>(err: E) -> Status
where
    E: std::error::Error,
{
    Status::internal(err.to_string())
}

impl TryFrom<proto::jobs::v1::JobType> for ora_backend::jobs::JobType {
    type Error = tonic::Status;

    fn try_from(value: proto::jobs::v1::JobType) -> Result<Self, Self::Error> {
        Ok(Self {
            id: JobTypeId::new(value.id).map_err(|e| Status::invalid_argument(e.to_string()))?,
            description: value.description,
            input_schema_json: value.input_schema_json,
            output_schema_json: value.output_schema_json,
        })
    }
}

impl From<ora_backend::jobs::JobType> for proto::jobs::v1::JobType {
    fn from(value: ora_backend::jobs::JobType) -> Self {
        Self {
            id: value.id.into_inner(),
            description: value.description,
            input_schema_json: value.input_schema_json,
            output_schema_json: value.output_schema_json,
        }
    }
}

impl TryFrom<proto::admin::v1::JobFilters> for ora_backend::jobs::JobFilters {
    type Error = tonic::Status;

    fn try_from(value: proto::admin::v1::JobFilters) -> Result<Self, Self::Error> {
        Ok(Self {
            execution_statuses: if value.execution_statuses.is_empty() {
                None
            } else {
                Some(
                    value
                        .execution_statuses()
                        .map(Into::into)
                        .collect::<Vec<_>>(),
                )
            },
            job_ids: if value.job_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .job_ids
                        .into_iter()
                        .map(|id| {
                            id.parse::<Uuid>().map(Into::into).map_err(|e| {
                                Status::invalid_argument(format!("invalid job ID '{id}': {e}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            job_type_ids: if value.job_type_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .job_type_ids
                        .into_iter()
                        .map(|id| {
                            JobTypeId::new(id).map_err(|e| Status::invalid_argument(e.to_string()))
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            executor_ids: if value.executor_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .executor_ids
                        .into_iter()
                        .map(|id| {
                            id.parse::<Uuid>().map(Into::into).map_err(|e| {
                                Status::invalid_argument(format!("invalid executor ID '{id}': {e}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            target_execution_time: match value.target_execution_time {
                Some(range) => Some(range.try_into()?),
                None => None,
            },
            created_at: match value.created_at {
                Some(range) => Some(range.try_into()?),
                None => None,
            },
            labels: if value.labels.is_empty() {
                None
            } else {
                Some(value.labels.into_iter().map(Into::into).collect::<Vec<_>>())
            },
            execution_ids: if value.execution_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .execution_ids
                        .into_iter()
                        .map(|id| {
                            id.parse::<Uuid>().map(Into::into).map_err(|e| {
                                Status::invalid_argument(format!(
                                    "invalid execution ID '{id}': {e}"
                                ))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            schedule_ids: if value.schedule_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .schedule_ids
                        .into_iter()
                        .map(|id| {
                            id.parse::<Uuid>().map(Into::into).map_err(|e| {
                                Status::invalid_argument(format!("invalid schedule ID '{id}': {e}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
        })
    }
}

impl TryFrom<proto::admin::v1::JobOrderBy> for Option<ora_backend::jobs::JobOrderBy> {
    type Error = tonic::Status;

    fn try_from(value: proto::admin::v1::JobOrderBy) -> Result<Self, Self::Error> {
        match value {
            proto::admin::v1::JobOrderBy::Unspecified => Ok(None),
            proto::admin::v1::JobOrderBy::CreatedAtAsc => {
                Ok(Some(ora_backend::jobs::JobOrderBy::CreatedAtAsc))
            }
            proto::admin::v1::JobOrderBy::CreatedAtDesc => {
                Ok(Some(ora_backend::jobs::JobOrderBy::CreatedAtDesc))
            }
            proto::admin::v1::JobOrderBy::TargetExecutionTimeAsc => {
                Ok(Some(ora_backend::jobs::JobOrderBy::TargetExecutionTimeAsc))
            }
            proto::admin::v1::JobOrderBy::TargetExecutionTimeDesc => {
                Ok(Some(ora_backend::jobs::JobOrderBy::TargetExecutionTimeDesc))
            }
        }
    }
}

impl TryFrom<proto::common::v1::TimeRange> for ora_backend::common::TimeRange {
    type Error = tonic::Status;

    fn try_from(value: proto::common::v1::TimeRange) -> Result<Self, Self::Error> {
        Ok(Self {
            start: match value.start {
                Some(ts) => {
                    Some(ts.try_into().map_err(|e| {
                        Status::invalid_argument(format!("invalid start time: {e}"))
                    })?)
                }
                None => None,
            },
            end: match value.end {
                Some(ts) => Some(
                    ts.try_into()
                        .map_err(|e| Status::invalid_argument(format!("invalid end time: {e}")))?,
                ),
                None => None,
            },
        })
    }
}

impl From<ora_backend::common::TimeRange> for proto::common::v1::TimeRange {
    fn from(value: ora_backend::common::TimeRange) -> Self {
        Self {
            start: value.start.map(Into::into),
            end: value.end.map(Into::into),
        }
    }
}

impl From<proto::common::v1::Label> for ora_backend::common::Label {
    fn from(value: proto::common::v1::Label) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<ora_backend::common::Label> for proto::common::v1::Label {
    fn from(value: ora_backend::common::Label) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<proto::common::v1::LabelFilter> for ora_backend::common::LabelFilter {
    fn from(value: proto::common::v1::LabelFilter) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<ora_backend::common::LabelFilter> for proto::common::v1::LabelFilter {
    fn from(value: ora_backend::common::LabelFilter) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl TryFrom<proto::jobs::v1::Job> for ora_backend::jobs::JobDefinition {
    type Error = tonic::Status;

    fn try_from(value: proto::jobs::v1::Job) -> Result<Self, Self::Error> {
        Ok(Self {
            job_type_id: JobTypeId::new(value.job_type_id)
                .map_err(|e| Status::invalid_argument(e.to_string()))?,
            target_execution_time: value
                .target_execution_time
                .ok_or_else(|| Status::invalid_argument("missing target_execution_time"))?
                .try_into()
                .map_err(|e| {
                    Status::invalid_argument(format!("invalid target_execution_time: {e}"))
                })?,
            input_payload_json: value.input_payload_json,
            timeout_policy: value.timeout_policy.unwrap_or_default().into(),
            retry_policy: value.retry_policy.unwrap_or_default().into(),
            labels: value.labels.into_iter().map(Into::into).collect(),
        })
    }
}

impl From<ora_backend::jobs::JobDefinition> for proto::jobs::v1::Job {
    fn from(value: ora_backend::jobs::JobDefinition) -> Self {
        Self {
            job_type_id: value.job_type_id.into_inner(),
            target_execution_time: Some(value.target_execution_time.into()),
            input_payload_json: value.input_payload_json,
            timeout_policy: Some(value.timeout_policy.into()),
            retry_policy: Some(value.retry_policy.into()),
            labels: value.labels.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<proto::jobs::v1::TimeoutPolicy> for ora_backend::jobs::TimeoutPolicy {
    fn from(value: proto::jobs::v1::TimeoutPolicy) -> Self {
        Self {
            timeout: value
                .timeout
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            base_time: value.base_time().into(),
        }
    }
}

impl From<ora_backend::jobs::TimeoutPolicy> for proto::jobs::v1::TimeoutPolicy {
    fn from(value: ora_backend::jobs::TimeoutPolicy) -> Self {
        Self {
            timeout: Some(prost_types::Duration::try_from(value.timeout).unwrap_or_default()),
            base_time: proto::jobs::v1::TimeoutBaseTime::from(value.base_time).into(),
        }
    }
}

impl From<proto::jobs::v1::TimeoutBaseTime> for ora_backend::jobs::TimeoutBaseTime {
    fn from(value: proto::jobs::v1::TimeoutBaseTime) -> Self {
        match value {
            proto::jobs::v1::TimeoutBaseTime::Unspecified
            | proto::jobs::v1::TimeoutBaseTime::StartTime => {
                ora_backend::jobs::TimeoutBaseTime::StartTime
            }
            proto::jobs::v1::TimeoutBaseTime::TargetExecutionTime => {
                ora_backend::jobs::TimeoutBaseTime::TargetExecutionTime
            }
        }
    }
}

impl From<ora_backend::jobs::TimeoutBaseTime> for proto::jobs::v1::TimeoutBaseTime {
    fn from(value: ora_backend::jobs::TimeoutBaseTime) -> Self {
        match value {
            ora_backend::jobs::TimeoutBaseTime::StartTime => {
                proto::jobs::v1::TimeoutBaseTime::StartTime
            }
            ora_backend::jobs::TimeoutBaseTime::TargetExecutionTime => {
                proto::jobs::v1::TimeoutBaseTime::TargetExecutionTime
            }
        }
    }
}

impl From<proto::jobs::v1::RetryPolicy> for ora_backend::jobs::RetryPolicy {
    fn from(value: proto::jobs::v1::RetryPolicy) -> Self {
        Self {
            retries: value.retries,
            backoff_duration: value
                .backoff_duration
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            max_backoff_duration: value
                .max_backoff_duration
                .map(|d| d.try_into().unwrap_or_default()),
            backoff_strategy: value.backoff_strategy().into(),
        }
    }
}

impl From<ora_backend::jobs::RetryPolicy> for proto::jobs::v1::RetryPolicy {
    fn from(value: ora_backend::jobs::RetryPolicy) -> Self {
        Self {
            retries: value.retries,
            backoff_duration: Some(
                prost_types::Duration::try_from(value.backoff_duration).unwrap_or_default(),
            ),
            max_backoff_duration: value
                .max_backoff_duration
                .map(|d| prost_types::Duration::try_from(d).unwrap_or_default()),
            backoff_strategy: proto::jobs::v1::BackoffStrategy::from(value.backoff_strategy).into(),
        }
    }
}

impl From<proto::jobs::v1::BackoffStrategy> for ora_backend::jobs::BackoffStrategy {
    fn from(value: proto::jobs::v1::BackoffStrategy) -> Self {
        match value {
            proto::jobs::v1::BackoffStrategy::Unspecified
            | proto::jobs::v1::BackoffStrategy::Fixed => ora_backend::jobs::BackoffStrategy::Fixed,
            proto::jobs::v1::BackoffStrategy::Exponential => {
                ora_backend::jobs::BackoffStrategy::Exponential
            }
        }
    }
}

impl From<ora_backend::jobs::BackoffStrategy> for proto::jobs::v1::BackoffStrategy {
    fn from(value: ora_backend::jobs::BackoffStrategy) -> Self {
        match value {
            ora_backend::jobs::BackoffStrategy::Fixed => proto::jobs::v1::BackoffStrategy::Fixed,
            ora_backend::jobs::BackoffStrategy::Exponential => {
                proto::jobs::v1::BackoffStrategy::Exponential
            }
        }
    }
}

impl From<ora_backend::executions::ExecutionStatus> for proto::admin::v1::ExecutionStatus {
    fn from(value: ora_backend::executions::ExecutionStatus) -> Self {
        match value {
            ora_backend::executions::ExecutionStatus::Pending => {
                proto::admin::v1::ExecutionStatus::Pending
            }
            ora_backend::executions::ExecutionStatus::InProgress => {
                proto::admin::v1::ExecutionStatus::InProgress
            }
            ora_backend::executions::ExecutionStatus::Succeeded => {
                proto::admin::v1::ExecutionStatus::Succeeded
            }
            ora_backend::executions::ExecutionStatus::Failed => {
                proto::admin::v1::ExecutionStatus::Failed
            }
            ora_backend::executions::ExecutionStatus::Cancelled => {
                proto::admin::v1::ExecutionStatus::Cancelled
            }
        }
    }
}

impl From<proto::admin::v1::ExecutionStatus> for ora_backend::executions::ExecutionStatus {
    fn from(value: proto::admin::v1::ExecutionStatus) -> Self {
        match value {
            proto::admin::v1::ExecutionStatus::Pending
            | proto::admin::v1::ExecutionStatus::Unspecified => {
                ora_backend::executions::ExecutionStatus::Pending
            }
            proto::admin::v1::ExecutionStatus::InProgress => {
                ora_backend::executions::ExecutionStatus::InProgress
            }
            proto::admin::v1::ExecutionStatus::Succeeded => {
                ora_backend::executions::ExecutionStatus::Succeeded
            }
            proto::admin::v1::ExecutionStatus::Failed => {
                ora_backend::executions::ExecutionStatus::Failed
            }
            proto::admin::v1::ExecutionStatus::Cancelled => {
                ora_backend::executions::ExecutionStatus::Cancelled
            }
        }
    }
}

impl From<ora_backend::executions::ExecutionDetails> for proto::admin::v1::Execution {
    fn from(value: ora_backend::executions::ExecutionDetails) -> Self {
        Self {
            id: value.id.to_string(),
            executor_id: value.executor_id.map(|id| id.0.to_string()),
            status: proto::admin::v1::ExecutionStatus::from(value.status).into(),
            created_at: Some(value.created_at.into()),
            started_at: value.started_at.map(Into::into),
            succeeded_at: value.succeeded_at.map(Into::into),
            failed_at: value.failed_at.map(Into::into),
            cancelled_at: value.cancelled_at.map(Into::into),
            output_json: value.output_json,
            failure_reason: value.failure_reason,
            target_execution_time: Some(value.target_execution_time.into()),
        }
    }
}

impl From<ora_backend::jobs::JobDetails> for proto::admin::v1::Job {
    fn from(value: ora_backend::jobs::JobDetails) -> Self {
        Self {
            id: value.id.to_string(),
            job: Some(value.job.into()),
            schedule_id: value.schedule_id.map(|id| id.0.to_string()),
            created_at: Some(value.created_at.into()),
            executions: value.executions.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<proto::schedules::v1::Schedule> for ora_backend::schedules::ScheduleDefinition {
    type Error = tonic::Status;

    fn try_from(value: proto::schedules::v1::Schedule) -> Result<Self, Self::Error> {
        let scheduling_policy = value
            .scheduling
            .unwrap_or_default()
            .policy
            .ok_or_else(|| tonic::Status::invalid_argument("missing scheduling policy"))?;

        Ok(Self {
            scheduling: match scheduling_policy {
                proto::schedules::v1::scheduling_policy::Policy::Interval(fixed) => {
                    ora_backend::schedules::SchedulingPolicy::FixedInterval {
                        interval: fixed
                            .interval
                            .unwrap_or_default()
                            .try_into()
                            .unwrap_or_default(),
                        immediate: fixed.immediate,
                        missed: fixed.missed_time_policy().into(),
                    }
                }
                proto::schedules::v1::scheduling_policy::Policy::Cron(cron) => {
                    ora_backend::schedules::SchedulingPolicy::Cron {
                        missed: cron.missed_time_policy().into(),
                        expression: cron.cron_expression,
                        immediate: cron.immediate,
                    }
                }
            },
            job_template: value
                .job_template
                .ok_or_else(|| tonic::Status::invalid_argument("missing job_template"))?
                .try_into()?,
            labels: value.labels.into_iter().map(Into::into).collect(),
            time_range: value.time_range.unwrap_or_default().try_into()?,
        })
    }
}

impl From<ora_backend::schedules::ScheduleDefinition> for proto::schedules::v1::Schedule {
    fn from(value: ora_backend::schedules::ScheduleDefinition) -> Self {
        let policy = match value.scheduling {
            ora_backend::schedules::SchedulingPolicy::FixedInterval {
                interval,
                immediate,
                missed,
            } => proto::schedules::v1::scheduling_policy::Policy::Interval(
                proto::schedules::v1::SchedulingPolicyInterval {
                    interval: Some(prost_types::Duration::try_from(interval).unwrap_or_default()),
                    immediate,
                    missed_time_policy: proto::schedules::v1::MissedTimePolicy::from(missed).into(),
                },
            ),
            ora_backend::schedules::SchedulingPolicy::Cron {
                expression,
                immediate,
                missed,
            } => proto::schedules::v1::scheduling_policy::Policy::Cron(
                proto::schedules::v1::SchedulingPolicyCron {
                    cron_expression: expression,
                    immediate,
                    missed_time_policy: proto::schedules::v1::MissedTimePolicy::from(missed).into(),
                },
            ),
        };

        Self {
            scheduling: Some(proto::schedules::v1::SchedulingPolicy {
                policy: Some(policy),
            }),
            job_template: Some(value.job_template.into()),
            labels: value.labels.into_iter().map(Into::into).collect(),
            time_range: Some(value.time_range.into()),
        }
    }
}

impl From<proto::schedules::v1::MissedTimePolicy> for ora_backend::schedules::MissedTimePolicy {
    fn from(value: proto::schedules::v1::MissedTimePolicy) -> Self {
        match value {
            proto::schedules::v1::MissedTimePolicy::Skip
            | proto::schedules::v1::MissedTimePolicy::Unspecified => {
                ora_backend::schedules::MissedTimePolicy::Skip
            }
            proto::schedules::v1::MissedTimePolicy::Create => {
                ora_backend::schedules::MissedTimePolicy::Create
            }
        }
    }
}

impl From<ora_backend::schedules::MissedTimePolicy> for proto::schedules::v1::MissedTimePolicy {
    fn from(value: ora_backend::schedules::MissedTimePolicy) -> Self {
        match value {
            ora_backend::schedules::MissedTimePolicy::Skip => {
                proto::schedules::v1::MissedTimePolicy::Skip
            }
            ora_backend::schedules::MissedTimePolicy::Create => {
                proto::schedules::v1::MissedTimePolicy::Create
            }
        }
    }
}

impl TryFrom<proto::admin::v1::ScheduleFilters> for ora_backend::schedules::ScheduleFilters {
    type Error = tonic::Status;

    fn try_from(value: proto::admin::v1::ScheduleFilters) -> Result<Self, Self::Error> {
        Ok(Self {
            statuses: if value.statuses.is_empty() {
                None
            } else {
                Some(value.statuses().map(Into::into).collect::<Vec<_>>())
            },
            schedule_ids: if value.schedule_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .schedule_ids
                        .into_iter()
                        .map(|id| {
                            id.parse::<Uuid>().map(Into::into).map_err(|e| {
                                Status::invalid_argument(format!("invalid schedule ID '{id}': {e}"))
                            })
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
            created_at: match value.created_at {
                Some(range) => Some(range.try_into()?),
                None => None,
            },
            labels: if value.labels.is_empty() {
                None
            } else {
                Some(value.labels.into_iter().map(Into::into).collect::<Vec<_>>())
            },
            job_type_ids: if value.job_type_ids.is_empty() {
                None
            } else {
                Some(
                    value
                        .job_type_ids
                        .into_iter()
                        .map(|id| {
                            JobTypeId::new(id).map_err(|e| Status::invalid_argument(e.to_string()))
                        })
                        .collect::<Result<_, _>>()?,
                )
            },
        })
    }
}

impl From<ora_backend::schedules::ScheduleStatus> for proto::admin::v1::ScheduleStatus {
    fn from(value: ora_backend::schedules::ScheduleStatus) -> Self {
        match value {
            ora_backend::schedules::ScheduleStatus::Active => {
                proto::admin::v1::ScheduleStatus::Active
            }
            ora_backend::schedules::ScheduleStatus::Stopped => {
                proto::admin::v1::ScheduleStatus::Stopped
            }
        }
    }
}

impl From<proto::admin::v1::ScheduleStatus> for ora_backend::schedules::ScheduleStatus {
    fn from(value: proto::admin::v1::ScheduleStatus) -> Self {
        match value {
            proto::admin::v1::ScheduleStatus::Active
            | proto::admin::v1::ScheduleStatus::Unspecified => {
                ora_backend::schedules::ScheduleStatus::Active
            }
            proto::admin::v1::ScheduleStatus::Stopped => {
                ora_backend::schedules::ScheduleStatus::Stopped
            }
        }
    }
}

impl From<ora_backend::schedules::ScheduleDetails> for proto::admin::v1::Schedule {
    fn from(value: ora_backend::schedules::ScheduleDetails) -> Self {
        Self {
            id: value.id.to_string(),
            schedule: Some(value.schedule.into()),
            created_at: Some(value.created_at.into()),
            status: proto::admin::v1::ScheduleStatus::from(value.status).into(),
            stopped_at: value.stopped_at.map(Into::into),
        }
    }
}

impl TryFrom<proto::admin::v1::ScheduleOrderBy>
    for Option<ora_backend::schedules::ScheduleOrderBy>
{
    type Error = tonic::Status;

    fn try_from(value: proto::admin::v1::ScheduleOrderBy) -> Result<Self, Self::Error> {
        match value {
            proto::admin::v1::ScheduleOrderBy::Unspecified => Ok(None),
            proto::admin::v1::ScheduleOrderBy::CreatedAtAsc => {
                Ok(Some(ora_backend::schedules::ScheduleOrderBy::CreatedAtAsc))
            }
            proto::admin::v1::ScheduleOrderBy::CreatedAtDesc => {
                Ok(Some(ora_backend::schedules::ScheduleOrderBy::CreatedAtDesc))
            }
        }
    }
}
