use std::time::SystemTime;

use crate::{
    job_definition::{JobDefinition, RetryPolicy, TimeoutBaseTime, TimeoutPolicy},
    schedule_definition::{
        ScheduleDefinition, ScheduleDetails, ScheduleJobCreationPolicy, ScheduleJobTimingPolicy,
        ScheduleJobTimingPolicyCron, ScheduleJobTimingPolicyRepeat, ScheduleMissedTimePolicy,
        ScheduleTimeRange,
    },
};
use eyre::OptionExt;
use ora_proto::{common::v1, server::v1::Schedule};

impl From<v1::JobTimeoutPolicy> for TimeoutPolicy {
    fn from(policy: v1::JobTimeoutPolicy) -> Self {
        Self {
            timeout: policy.timeout.and_then(|d| d.try_into().ok()),
            base_time: policy.base_time().into(),
        }
    }
}

impl From<TimeoutPolicy> for v1::JobTimeoutPolicy {
    fn from(policy: TimeoutPolicy) -> Self {
        Self {
            timeout: policy.timeout.and_then(|v| v.try_into().ok()),
            base_time: v1::JobTimeoutBaseTime::from(policy.base_time).into(),
        }
    }
}

impl From<v1::JobTimeoutBaseTime> for TimeoutBaseTime {
    fn from(base_time: v1::JobTimeoutBaseTime) -> Self {
        match base_time {
            v1::JobTimeoutBaseTime::Unspecified | v1::JobTimeoutBaseTime::StartTime => {
                Self::StartTime
            }
            v1::JobTimeoutBaseTime::TargetExecutionTime => Self::TargetExecutionTime,
        }
    }
}

impl From<TimeoutBaseTime> for v1::JobTimeoutBaseTime {
    fn from(base_time: TimeoutBaseTime) -> Self {
        match base_time {
            TimeoutBaseTime::StartTime => v1::JobTimeoutBaseTime::StartTime,
            TimeoutBaseTime::TargetExecutionTime => v1::JobTimeoutBaseTime::TargetExecutionTime,
        }
    }
}

impl From<v1::JobRetryPolicy> for RetryPolicy {
    fn from(policy: v1::JobRetryPolicy) -> Self {
        Self {
            retries: policy.retries,
        }
    }
}

impl From<RetryPolicy> for v1::JobRetryPolicy {
    fn from(policy: RetryPolicy) -> Self {
        Self {
            retries: policy.retries,
        }
    }
}

impl From<JobDefinition> for v1::JobDefinition {
    fn from(definition: JobDefinition) -> Self {
        Self {
            job_type_id: definition.job_type_id.into(),
            target_execution_time: Some(definition.target_execution_time.into()),
            input_payload_json: definition.input_payload_json,
            labels: definition
                .labels
                .into_iter()
                .map(|(key, value)| v1::JobLabel { key, value })
                .collect(),
            timeout_policy: Some(definition.timeout_policy.into()),
            retry_policy: Some(definition.retry_policy.into()),
            metadata_json: definition.metadata_json,
        }
    }
}

impl From<v1::JobDefinition> for JobDefinition {
    fn from(definition: v1::JobDefinition) -> Self {
        Self {
            job_type_id: definition.job_type_id.into(),
            target_execution_time: definition
                .target_execution_time
                .and_then(|t| SystemTime::try_from(t).ok())
                .unwrap_or(SystemTime::UNIX_EPOCH),
            input_payload_json: definition.input_payload_json,
            labels: definition
                .labels
                .into_iter()
                .map(|label| (label.key, label.value))
                .collect(),
            timeout_policy: definition.timeout_policy.unwrap_or_default().into(),
            retry_policy: definition.retry_policy.unwrap_or_default().into(),
            metadata_json: definition.metadata_json,
        }
    }
}

impl From<ScheduleDefinition> for v1::ScheduleDefinition {
    fn from(definition: ScheduleDefinition) -> Self {
        Self {
            job_timing_policy: Some(definition.job_timing_policy.into()),
            job_creation_policy: Some(match definition.job_creation_policy {
                ScheduleJobCreationPolicy::JobDefinition(job_definition) => {
                    v1::ScheduleJobCreationPolicy {
                        job_creation: Some(
                            v1::schedule_job_creation_policy::JobCreation::JobDefinition(
                                job_definition.into(),
                            ),
                        ),
                    }
                }
            }),
            labels: definition
                .labels
                .into_iter()
                .map(|(key, value)| v1::ScheduleLabel { key, value })
                .collect(),
            time_range: definition.time_range.map(Into::into),
            metadata_json: definition.metadata_json,
        }
    }
}

impl From<v1::TimeRange> for ScheduleTimeRange {
    fn from(time_range: v1::TimeRange) -> Self {
        Self {
            start: time_range
                .start
                .and_then(|start_time| start_time.try_into().ok()),
            end: time_range.end.and_then(|end_time| end_time.try_into().ok()),
        }
    }
}

impl From<ScheduleTimeRange> for v1::TimeRange {
    fn from(time_range: ScheduleTimeRange) -> Self {
        Self {
            start: time_range.start.map(Into::into),
            end: time_range.end.map(Into::into),
        }
    }
}

impl From<ScheduleJobTimingPolicy> for v1::ScheduleJobTimingPolicy {
    fn from(policy: ScheduleJobTimingPolicy) -> Self {
        match policy {
            ScheduleJobTimingPolicy::Repeat(scheduling_policy_repeat) => {
                v1::ScheduleJobTimingPolicy {
                    job_timing: Some(v1::schedule_job_timing_policy::JobTiming::Repeat(
                        v1::ScheduleJobTimingPolicyRepeat {
                            interval: Some(scheduling_policy_repeat.interval.try_into().unwrap()),
                            immediate: scheduling_policy_repeat.immediate,
                            missed_time_policy: v1::ScheduleMissedTimePolicy::from(
                                scheduling_policy_repeat.missed_time_policy,
                            )
                            .into(),
                        },
                    )),
                }
            }
            ScheduleJobTimingPolicy::Cron(scheduling_policy_cron) => v1::ScheduleJobTimingPolicy {
                job_timing: Some(v1::schedule_job_timing_policy::JobTiming::Cron(
                    v1::ScheduleJobTimingPolicyCron {
                        cron_expression: scheduling_policy_cron.cron_expression,
                        immediate: scheduling_policy_cron.immediate,
                        missed_time_policy: v1::ScheduleMissedTimePolicy::from(
                            scheduling_policy_cron.missed_time_policy,
                        )
                        .into(),
                    },
                )),
            },
        }
    }
}

impl TryFrom<v1::ScheduleJobTimingPolicy> for ScheduleJobTimingPolicy {
    type Error = eyre::Report;

    fn try_from(policy: v1::ScheduleJobTimingPolicy) -> eyre::Result<Self> {
        Ok(match policy.job_timing.ok_or_eyre("missing job timing")? {
            v1::schedule_job_timing_policy::JobTiming::Repeat(scheduling_policy_repeat) => {
                ScheduleJobTimingPolicy::Repeat(ScheduleJobTimingPolicyRepeat {
                    missed_time_policy: scheduling_policy_repeat.missed_time_policy().into(),
                    interval: scheduling_policy_repeat
                        .interval
                        .ok_or_eyre("missing schedule policy interval")?
                        .try_into()?,
                    immediate: scheduling_policy_repeat.immediate,
                })
            }
            v1::schedule_job_timing_policy::JobTiming::Cron(scheduling_policy_cron) => {
                ScheduleJobTimingPolicy::Cron(ScheduleJobTimingPolicyCron {
                    missed_time_policy: scheduling_policy_cron.missed_time_policy().into(),
                    cron_expression: scheduling_policy_cron.cron_expression,
                    immediate: scheduling_policy_cron.immediate,
                })
            }
        })
    }
}

impl From<ScheduleMissedTimePolicy> for v1::ScheduleMissedTimePolicy {
    fn from(policy: ScheduleMissedTimePolicy) -> Self {
        match policy {
            ScheduleMissedTimePolicy::Skip => Self::Skip,
            ScheduleMissedTimePolicy::Create => Self::Create,
        }
    }
}

impl From<v1::ScheduleMissedTimePolicy> for ScheduleMissedTimePolicy {
    fn from(policy: v1::ScheduleMissedTimePolicy) -> Self {
        match policy {
            v1::ScheduleMissedTimePolicy::Skip | v1::ScheduleMissedTimePolicy::Unspecified => {
                Self::Skip
            }
            v1::ScheduleMissedTimePolicy::Create => Self::Create,
        }
    }
}

impl From<ScheduleJobCreationPolicy> for v1::ScheduleJobCreationPolicy {
    fn from(policy: ScheduleJobCreationPolicy) -> Self {
        match policy {
            ScheduleJobCreationPolicy::JobDefinition(job_definition) => Self {
                job_creation: Some(
                    v1::schedule_job_creation_policy::JobCreation::JobDefinition(
                        job_definition.into(),
                    ),
                ),
            },
        }
    }
}

impl TryFrom<v1::ScheduleJobCreationPolicy> for ScheduleJobCreationPolicy {
    type Error = eyre::Report;

    fn try_from(policy: v1::ScheduleJobCreationPolicy) -> eyre::Result<Self> {
        Ok(
            match policy.job_creation.ok_or_eyre("missing job creation")? {
                v1::schedule_job_creation_policy::JobCreation::JobDefinition(job_definition) => {
                    ScheduleJobCreationPolicy::JobDefinition(job_definition.into())
                }
            },
        )
    }
}

impl TryFrom<Schedule> for ScheduleDetails {
    type Error = eyre::Report;

    fn try_from(schedule: Schedule) -> eyre::Result<Self> {
        let schedule_definition = schedule
            .definition
            .ok_or_eyre("missing schedule definition")?;

        Ok(Self {
            id: schedule.id.parse()?,
            created_at: SystemTime::try_from(
                schedule.created_at.ok_or_eyre("missing created_at")?,
            )?,
            cancelled: schedule.cancelled,
            active: schedule.active,
            job_timing_policy: schedule_definition
                .job_timing_policy
                .ok_or_eyre("missing job timing policy")?
                .try_into()?,
            job_creation_policy: schedule_definition
                .job_creation_policy
                .ok_or_eyre("missing job creation policy")?
                .try_into()?,
            labels: schedule_definition
                .labels
                .into_iter()
                .map(|label| (label.key, label.value))
                .collect(),
            time_range: schedule_definition.time_range.map(Into::into),
            metadata_json: schedule_definition.metadata_json,
        })
    }
}
