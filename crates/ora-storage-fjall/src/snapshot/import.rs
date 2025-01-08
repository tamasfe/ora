use std::time::SystemTime;

use eyre::{Context, OptionExt};
use fjall::WriteTransaction;
use ora_proto::snapshot::v1::SnapshotData;
use uuid::Uuid;

use crate::{
    indexes::{JobExecutionIndexKey, LabelIndexKey, ScheduleJobIndexKey},
    models::{ExecutionData, JobData, JobTypeData, ScheduleData},
    partitions::Partitions,
};

/// Import a chunk of snapshot data into the storage.
///
/// In progress jobs and executions will not be imported,
/// instead active jobs and schedules will be imported and will
/// be marked as pending to achieve an at-least-once guarantee
/// for jobs during migration.
pub(super) fn import_snapshot_data<'a>(
    tx: &mut WriteTransaction<'a>,
    partitions: &'a Partitions,
    data: SnapshotData,
) -> eyre::Result<()> {
    for job_type in data.job_types {
        let Some(job_type) = job_type.job_type else {
            tracing::warn!("missing job type data in snapshot");
            continue;
        };

        partitions.job_types.write(tx).insert(
            &job_type.id,
            &JobTypeData {
                name: job_type.name.unwrap_or_default(),
                description: job_type.description.unwrap_or_default(),
                input_schema_json: job_type.input_schema_json,
                output_schema_json: job_type.output_schema_json,
            },
        );
    }

    for schedule in data.schedules {
        let active = schedule.marked_unschedulable_at.is_none();

        let schedule_id: Uuid = schedule.id.parse().wrap_err("invalid schedule id")?;
        let schedule_data = ScheduleData {
            id: schedule_id,
            job_type_id: schedule.job_type_id,
            cancelled_at: schedule
                .cancelled_at
                .map(SystemTime::try_from)
                .transpose()?,
            created_at: schedule
                .created_at
                .map(SystemTime::try_from)
                .transpose()?
                .ok_or_eyre("missing created_at")?,
            job_creation_policy: ora_storage::ScheduleJobCreationPolicy::try_from(
                schedule
                    .job_creation_policy
                    .ok_or_eyre("missing job_creation_policy")?,
            )?
            .into(),
            job_timing_policy: ora_storage::ScheduleJobTimingPolicy::try_from(
                schedule
                    .job_timing_policy
                    .ok_or_eyre("missing job_timing_policy")?,
            )?
            .into(),
            labels: schedule
                .labels
                .into_iter()
                .map(|s| (s.key, s.value))
                .collect(),
            marked_unschedulable_at: schedule
                .marked_unschedulable_at
                .map(SystemTime::try_from)
                .transpose()?,
            time_range: schedule
                .time_range
                .map(|tr| {
                    Result::<_, eyre::Report>::Ok(ora_storage::ScheduleTimeRange {
                        start: tr.start.map(SystemTime::try_from).transpose()?,
                        end: tr.end.map(SystemTime::try_from).transpose()?,
                    })
                })
                .transpose()?
                .map(Into::into),
            metadata_json: schedule.metadata_json,
        };

        if active {
            partitions
                .active_schedules
                .write(tx)
                .insert(&schedule_id, &schedule_data);
        } else {
            partitions
                .inactive_schedules
                .write(tx)
                .insert(&schedule_id, &schedule_data);
        }

        // Update indexes
        for (key, value) in schedule_data.labels {
            partitions
                .idx_schedule_labels
                .write(tx)
                .insert(&LabelIndexKey::new(&key, &value, schedule_id), &());
        }

        if active {
            partitions
                .idx_pending_schedules
                .write(tx)
                .insert(&schedule_id, &());
        }
    }

    for job in data.jobs {
        let active = job.marked_unschedulable_at.is_none();

        if active && job.schedule_id.is_some() {
            tracing::debug!("active job with a schedule will not be imported");
            continue;
        }

        let job_id: Uuid = job.id.parse().wrap_err("invalid job id")?;
        let job_data = JobData {
            id: job_id,
            schedule_id: job.schedule_id.map(|s| s.parse()).transpose()?,
            created_at: job
                .created_at
                .map(SystemTime::try_from)
                .transpose()?
                .ok_or_eyre("missing created_at")?,
            job_type_id: job.job_type_id,
            target_execution_time: job
                .target_execution_time
                .map(SystemTime::try_from)
                .transpose()?
                .ok_or_eyre("missing target_execution_time")?,
            retry_policy: ora_storage::JobRetryPolicy::from(
                job.retry_policy.ok_or_eyre("missing retry_policy")?,
            )
            .into(),
            timeout_policy: ora_storage::JobTimeoutPolicy::from(
                job.timeout_policy.ok_or_eyre("missing timeout_policy")?,
            )
            .into(),
            labels: job.labels.into_iter().map(|s| (s.key, s.value)).collect(),
            input_payload_json: job.input_payload_json,
            metadata_json: job.metadata_json,
            cancelled_at: job.cancelled_at.map(SystemTime::try_from).transpose()?,
            marked_unschedulable_at: job
                .marked_unschedulable_at
                .map(SystemTime::try_from)
                .transpose()?,
        };

        if active {
            partitions.active_jobs.write(tx).insert(&job_id, &job_data);
        } else {
            partitions
                .inactive_jobs
                .write(tx)
                .insert(&job_id, &job_data);
        }

        // Update indexes
        for (key, value) in job_data.labels {
            partitions
                .idx_job_labels
                .write(tx)
                .insert(&LabelIndexKey::new(&key, &value, job_id), &());
        }

        if let Some(schedule_id) = job_data.schedule_id {
            partitions
                .idx_job_schedule
                .write(tx)
                .insert(&job_id, &schedule_id);

            partitions
                .idx_schedule_jobs
                .write(tx)
                .insert(&ScheduleJobIndexKey::new(schedule_id, job_id), &());
        }

        if active {
            partitions.idx_pending_jobs.write(tx).insert(&job_id, &());
        }
    }

    for execution in data.executions {
        let execution_id: Uuid = execution.id.parse().wrap_err("invalid execution id")?;

        if execution.succeeded_at.is_none() && execution.failed_at.is_none() {
            tracing::debug!("execution in progress will not be imported");
            continue;
        }

        let execution_data = ExecutionData {
            id: execution_id,
            job_id: execution.job_id.parse().wrap_err("invalid job id")?,
            executor_id: execution.executor_id.map(|s| s.parse()).transpose()?,
            created_at: execution
                .created_at
                .map(SystemTime::try_from)
                .transpose()?
                .ok_or_eyre("missing created_at")?,
            ready_at: execution.ready_at.map(SystemTime::try_from).transpose()?,
            assigned_at: execution
                .assigned_at
                .map(SystemTime::try_from)
                .transpose()?,
            started_at: execution.started_at.map(SystemTime::try_from).transpose()?,
            succeeded_at: execution
                .succeeded_at
                .map(SystemTime::try_from)
                .transpose()?,
            failed_at: execution.failed_at.map(SystemTime::try_from).transpose()?,
            output_payload_json: execution.output_payload_json,
            failure_reason: execution.failure_reason,
            target_execution_time: execution
                .target_execution_time
                .map(SystemTime::try_from)
                .transpose()?
                .ok_or_eyre("missing target_execution_time")?,
        };

        if execution_data.succeeded_at.is_some() {
            partitions
                .succeeded_executions
                .write(tx)
                .insert(&execution_id, &execution_data);
        } else if execution_data.failed_at.is_some() {
            partitions
                .failed_executions
                .write(tx)
                .insert(&execution_id, &execution_data);
        } else {
            unreachable!();
        }

        // Update indexes
        partitions.idx_job_executions.write(tx).insert(
            &JobExecutionIndexKey::new(execution_data.job_id, execution_id),
            &(),
        );
    }

    Ok(())
}
