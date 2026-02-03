use crate::{
    JobDefinition, JobType, ScheduleDefinition,
    admin::{jobs::JobFilters, schedules::ScheduleFilters},
    common::LabelFilter,
    execution::ExecutionStatus,
    job::{RetryPolicy, TimeoutPolicy},
    proto::{self, common::v1::TimeRange},
    schedule::SchedulingPolicy,
};

impl From<JobFilters> for proto::admin::v1::JobFilters {
    fn from(value: JobFilters) -> Self {
        Self {
            job_ids: value
                .job_ids
                .into_iter()
                .flatten()
                .map(|v| v.to_string())
                .collect(),
            job_type_ids: value
                .job_type_ids
                .into_iter()
                .flatten()
                .map(|v| v.into_inner().into())
                .collect(),
            schedule_ids: value
                .schedule_ids
                .into_iter()
                .flatten()
                .map(|v| v.to_string())
                .collect(),
            executor_ids: value
                .executor_ids
                .into_iter()
                .flatten()
                .map(|v| v.to_string())
                .collect(),
            execution_ids: value
                .execution_ids
                .into_iter()
                .flatten()
                .map(|v| v.to_string())
                .collect(),
            execution_statuses: value
                .execution_statuses
                .into_iter()
                .flatten()
                .map(|v| proto::admin::v1::ExecutionStatus::from(v) as i32)
                .collect(),
            target_execution_time: value.target_execution_time.map(Into::into),
            created_at: value.created_at.map(Into::into),
            labels: value
                .labels
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<ExecutionStatus> for proto::admin::v1::ExecutionStatus {
    fn from(value: ExecutionStatus) -> Self {
        match value {
            ExecutionStatus::Pending => proto::admin::v1::ExecutionStatus::Pending,
            ExecutionStatus::InProgress => proto::admin::v1::ExecutionStatus::InProgress,
            ExecutionStatus::Succeeded => proto::admin::v1::ExecutionStatus::Succeeded,
            ExecutionStatus::Failed => proto::admin::v1::ExecutionStatus::Failed,
            ExecutionStatus::Cancelled => proto::admin::v1::ExecutionStatus::Cancelled,
        }
    }
}

impl From<proto::admin::v1::ExecutionStatus> for ExecutionStatus {
    fn from(value: proto::admin::v1::ExecutionStatus) -> Self {
        match value {
            proto::admin::v1::ExecutionStatus::Pending
            | proto::admin::v1::ExecutionStatus::Unspecified => ExecutionStatus::Pending,
            proto::admin::v1::ExecutionStatus::InProgress => ExecutionStatus::InProgress,
            proto::admin::v1::ExecutionStatus::Succeeded => ExecutionStatus::Succeeded,
            proto::admin::v1::ExecutionStatus::Failed => ExecutionStatus::Failed,
            proto::admin::v1::ExecutionStatus::Cancelled => ExecutionStatus::Cancelled,
        }
    }
}

impl From<TimeRange> for crate::common::TimeRange {
    fn from(value: TimeRange) -> Self {
        Self {
            start: value.start.and_then(|t| t.try_into().ok()),
            end: value.end.and_then(|t| t.try_into().ok()),
        }
    }
}

impl From<crate::common::TimeRange> for TimeRange {
    fn from(value: crate::common::TimeRange) -> Self {
        Self {
            start: value.start.map(Into::into),
            end: value.end.map(Into::into),
        }
    }
}

impl From<LabelFilter> for proto::common::v1::LabelFilter {
    fn from(value: LabelFilter) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<TimeoutPolicy> for proto::jobs::v1::TimeoutPolicy {
    fn from(value: TimeoutPolicy) -> Self {
        Self {
            timeout: value.timeout.try_into().ok(),
            base_time: proto::jobs::v1::TimeoutBaseTime::from(value.base_time) as _,
        }
    }
}

impl From<proto::jobs::v1::TimeoutPolicy> for TimeoutPolicy {
    fn from(value: proto::jobs::v1::TimeoutPolicy) -> Self {
        Self {
            timeout: value
                .timeout
                .and_then(|d| d.try_into().ok())
                .unwrap_or_default(),
            base_time: proto::jobs::v1::TimeoutBaseTime::try_from(value.base_time)
                .unwrap_or(proto::jobs::v1::TimeoutBaseTime::Unspecified)
                .into(),
        }
    }
}

impl From<crate::job::TimeoutBaseTime> for proto::jobs::v1::TimeoutBaseTime {
    fn from(value: crate::job::TimeoutBaseTime) -> Self {
        match value {
            crate::job::TimeoutBaseTime::TargetExecutionTime => {
                proto::jobs::v1::TimeoutBaseTime::TargetExecutionTime
            }
            crate::job::TimeoutBaseTime::StartTime => proto::jobs::v1::TimeoutBaseTime::StartTime,
        }
    }
}

impl From<proto::jobs::v1::TimeoutBaseTime> for crate::job::TimeoutBaseTime {
    fn from(value: proto::jobs::v1::TimeoutBaseTime) -> Self {
        match value {
            proto::jobs::v1::TimeoutBaseTime::Unspecified
            | proto::jobs::v1::TimeoutBaseTime::TargetExecutionTime => {
                crate::job::TimeoutBaseTime::TargetExecutionTime
            }
            proto::jobs::v1::TimeoutBaseTime::StartTime => crate::job::TimeoutBaseTime::StartTime,
        }
    }
}

impl From<RetryPolicy> for proto::jobs::v1::RetryPolicy {
    fn from(value: RetryPolicy) -> Self {
        Self {
            retries: value.retries,
        }
    }
}

impl From<proto::jobs::v1::RetryPolicy> for RetryPolicy {
    fn from(value: proto::jobs::v1::RetryPolicy) -> Self {
        Self {
            retries: value.retries,
        }
    }
}

impl From<proto::schedules::v1::MissedTimePolicy> for crate::schedule::MissedTimePolicy {
    fn from(value: proto::schedules::v1::MissedTimePolicy) -> Self {
        match value {
            proto::schedules::v1::MissedTimePolicy::Skip
            | proto::schedules::v1::MissedTimePolicy::Unspecified => {
                crate::schedule::MissedTimePolicy::Skip
            }
            proto::schedules::v1::MissedTimePolicy::Create => {
                crate::schedule::MissedTimePolicy::Create
            }
        }
    }
}

impl From<crate::schedule::MissedTimePolicy> for proto::schedules::v1::MissedTimePolicy {
    fn from(value: crate::schedule::MissedTimePolicy) -> Self {
        match value {
            crate::schedule::MissedTimePolicy::Skip => proto::schedules::v1::MissedTimePolicy::Skip,
            crate::schedule::MissedTimePolicy::Create => {
                proto::schedules::v1::MissedTimePolicy::Create
            }
        }
    }
}

impl From<crate::admin::schedules::ScheduleStatus> for proto::admin::v1::ScheduleStatus {
    fn from(value: crate::admin::schedules::ScheduleStatus) -> Self {
        match value {
            crate::admin::schedules::ScheduleStatus::Active => {
                proto::admin::v1::ScheduleStatus::Active
            }
            crate::admin::schedules::ScheduleStatus::Stopped => {
                proto::admin::v1::ScheduleStatus::Stopped
            }
        }
    }
}

impl From<proto::admin::v1::ScheduleStatus> for crate::admin::schedules::ScheduleStatus {
    fn from(value: proto::admin::v1::ScheduleStatus) -> Self {
        match value {
            proto::admin::v1::ScheduleStatus::Active
            | proto::admin::v1::ScheduleStatus::Unspecified => {
                crate::admin::schedules::ScheduleStatus::Active
            }
            proto::admin::v1::ScheduleStatus::Stopped => {
                crate::admin::schedules::ScheduleStatus::Stopped
            }
        }
    }
}

impl From<SchedulingPolicy> for proto::schedules::v1::SchedulingPolicy {
    fn from(value: SchedulingPolicy) -> Self {
        match value {
            SchedulingPolicy::FixedInterval {
                interval,
                immediate,
                missed,
            } => Self {
                policy: Some(proto::schedules::v1::scheduling_policy::Policy::Interval(
                    proto::schedules::v1::SchedulingPolicyInterval {
                        interval: Some(interval.try_into().unwrap()),
                        immediate,
                        missed_time_policy: proto::schedules::v1::MissedTimePolicy::from(missed)
                            as i32,
                    },
                )),
            },
            SchedulingPolicy::Cron {
                expression,
                immediate,
                missed,
            } => Self {
                policy: Some(proto::schedules::v1::scheduling_policy::Policy::Cron(
                    proto::schedules::v1::SchedulingPolicyCron {
                        cron_expression: expression,
                        immediate,
                        missed_time_policy: proto::schedules::v1::MissedTimePolicy::from(missed)
                            as i32,
                    },
                )),
            },
        }
    }
}

impl TryFrom<proto::schedules::v1::SchedulingPolicy> for SchedulingPolicy {
    type Error = eyre::ErrReport;

    fn try_from(value: proto::schedules::v1::SchedulingPolicy) -> Result<Self, Self::Error> {
        match value.policy {
            Some(proto::schedules::v1::scheduling_policy::Policy::Interval(interval)) => {
                Ok(SchedulingPolicy::FixedInterval {
                    interval: interval
                        .interval
                        .ok_or_else(|| eyre::eyre!("missing interval"))?
                        .try_into()?,
                    immediate: interval.immediate,
                    missed: proto::schedules::v1::MissedTimePolicy::try_from(
                        interval.missed_time_policy,
                    )?
                    .into(),
                })
            }
            Some(proto::schedules::v1::scheduling_policy::Policy::Cron(cron)) => {
                Ok(SchedulingPolicy::Cron {
                    expression: cron.cron_expression,
                    immediate: cron.immediate,
                    missed: proto::schedules::v1::MissedTimePolicy::try_from(
                        cron.missed_time_policy,
                    )?
                    .into(),
                })
            }
            None => Err(eyre::eyre!("missing scheduling policy")),
        }
    }
}

impl From<ScheduleFilters> for proto::admin::v1::ScheduleFilters {
    fn from(value: ScheduleFilters) -> Self {
        Self {
            schedule_ids: value
                .schedule_ids
                .into_iter()
                .flatten()
                .map(|v| v.to_string())
                .collect(),
            job_type_ids: value
                .job_type_ids
                .into_iter()
                .flatten()
                .map(|v| v.into_inner().into())
                .collect(),
            statuses: value
                .statuses
                .into_iter()
                .flatten()
                .map(|v| proto::admin::v1::ScheduleStatus::from(v) as i32)
                .collect(),
            created_at: value.created_at.map(Into::into),
            labels: value
                .labels
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl<J> TryFrom<ScheduleDefinition<J>> for proto::schedules::v1::Schedule
where
    J: JobType,
{
    type Error = crate::Error;

    fn try_from(schedule: ScheduleDefinition<J>) -> Result<Self, Self::Error> {
        Ok(proto::schedules::v1::Schedule {
            scheduling: Some(schedule.scheduling.into()),
            job_template: Some(proto::jobs::v1::Job {
                job_type_id: J::job_type_id().into_inner().into(),
                target_execution_time: Some(schedule.job_template.target_execution_time.into()),
                input_payload_json: serde_json::to_string(&schedule.job_template.input)?,
                labels: schedule
                    .job_template
                    .labels
                    .into_iter()
                    .map(|(key, value)| proto::common::v1::Label { key, value })
                    .collect(),
                timeout_policy: Some(schedule.job_template.timeout_policy.into()),
                retry_policy: Some(schedule.job_template.retry_policy.into()),
            }),
            labels: schedule
                .labels
                .into_iter()
                .map(|(key, value)| proto::common::v1::Label { key, value })
                .collect(),
            time_range: Some(schedule.time_range.into()),
        })
    }
}

impl<J> TryFrom<JobDefinition<J>> for proto::jobs::v1::Job
where
    J: JobType,
{
    type Error = crate::Error;

    fn try_from(job: JobDefinition<J>) -> Result<Self, Self::Error> {
        Ok(proto::jobs::v1::Job {
            job_type_id: J::job_type_id().into_inner().into(),
            target_execution_time: Some(job.target_execution_time.into()),
            input_payload_json: serde_json::to_string(&job.input)?,
            labels: job
                .labels
                .into_iter()
                .map(|(key, value)| proto::common::v1::Label { key, value })
                .collect(),
            timeout_policy: Some(job.timeout_policy.into()),
            retry_policy: Some(job.retry_policy.into()),
        })
    }
}
