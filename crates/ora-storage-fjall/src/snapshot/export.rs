use fjall::ReadTransaction;
use ora_proto::{
    common::v1::{JobType, ScheduleLabel},
    snapshot::v1::{ExportedJob, ExportedJobType, ExportedSchedule, SnapshotData},
};

use crate::{partitions::Partitions, util::deserialize_systemtime};

pub(super) fn export_snapshot(
    tx: &ReadTransaction,
    partitions: &Partitions,
    snd: flume::Sender<eyre::Result<SnapshotData>>,
) -> eyre::Result<()> {
    export_job_types(tx, partitions, &snd)?;
    export_schedules(tx, partitions, &snd)?;
    export_jobs(tx, partitions, &snd)?;
    export_executions(tx, partitions, &snd)?;
    Ok(())
}

#[tracing::instrument(skip_all)]
fn export_job_types(
    tx: &ReadTransaction,
    partitions: &Partitions,
    snd: &flume::Sender<eyre::Result<SnapshotData>>,
) -> eyre::Result<()> {
    const CHUNK_SIZE: usize = 1000;

    let mut job_types: Vec<ExportedJobType> = Vec::with_capacity(CHUNK_SIZE);

    for job_type_data in partitions.job_types.read(tx).iter() {
        let (job_type_id, job_type_data) = job_type_data?;
        let job_type_data = job_type_data.value();

        job_types.push(ExportedJobType {
            job_type: Some(JobType {
                id: job_type_id.value().into(),
                name: Some(job_type_data.name.as_str().into()),
                description: Some(job_type_data.description.as_str().into()),
                input_schema_json: job_type_data
                    .input_schema_json
                    .as_ref()
                    .map(|s| s.as_str().into()),
                output_schema_json: job_type_data
                    .output_schema_json
                    .as_ref()
                    .map(|s| s.as_str().into()),
            }),
        });

        if job_types.len() >= CHUNK_SIZE {
            if snd
                .send(Ok(SnapshotData {
                    job_types: job_types.clone(),
                    ..Default::default()
                }))
                .is_err()
            {
                return Ok(());
            }

            job_types.clear();
        }
    }

    if !job_types.is_empty()
        && snd
            .send(Ok(SnapshotData {
                job_types,
                ..Default::default()
            }))
            .is_err()
    {
        return Ok(());
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
fn export_schedules(
    tx: &ReadTransaction,
    partitions: &Partitions,
    snd: &flume::Sender<eyre::Result<SnapshotData>>,
) -> eyre::Result<()> {
    const CHUNK_SIZE: usize = 1000;

    let mut schedules = Vec::with_capacity(CHUNK_SIZE);

    let schedule_partitions = [&partitions.active_schedules, &partitions.inactive_schedules];

    for partition in schedule_partitions {
        for schedule_data in partition.read(tx).iter() {
            let (schedule_id, schedule_data) = schedule_data?;
            let schedule_data = schedule_data.value();

            schedules.push(ExportedSchedule {
                id: schedule_id.value().into(),
                created_at: Some(deserialize_systemtime(schedule_data.created_at).into()),
                job_type_id: schedule_data
                    .job_type_id
                    .as_ref()
                    .map(|s| s.as_str().into()),
                labels: schedule_data
                    .labels
                    .iter()
                    .map(|(k, v)| ScheduleLabel {
                        key: k.as_str().into(),
                        value: v.as_str().into(),
                    })
                    .collect(),
                marked_unschedulable_at: schedule_data
                    .marked_unschedulable_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                cancelled_at: schedule_data
                    .cancelled_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                job_timing_policy: Some(
                    ora_storage::ScheduleJobTimingPolicy::from(
                        deserialize!(&schedule_data.job_timing_policy).unwrap(),
                    )
                    .into(),
                ),
                job_creation_policy: Some(
                    ora_storage::ScheduleJobCreationPolicy::from(
                        deserialize!(&schedule_data.job_creation_policy).unwrap(),
                    )
                    .into(),
                ),
                time_range: schedule_data
                    .time_range
                    .as_ref()
                    .map(|t| ora_storage::ScheduleTimeRange::from(deserialize!(t).unwrap()).into()),
                metadata_json: schedule_data
                    .metadata_json
                    .as_ref()
                    .map(|s| s.as_str().into()),
            });

            if schedules.len() >= CHUNK_SIZE {
                if snd
                    .send(Ok(SnapshotData {
                        schedules: schedules.clone(),
                        ..Default::default()
                    }))
                    .is_err()
                {
                    return Ok(());
                }

                schedules.clear();
            }
        }
    }

    if !schedules.is_empty()
        && snd
            .send(Ok(SnapshotData {
                schedules,
                ..Default::default()
            }))
            .is_err()
    {
        return Ok(());
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
fn export_jobs(
    tx: &ReadTransaction,
    partitions: &Partitions,
    snd: &flume::Sender<eyre::Result<SnapshotData>>,
) -> eyre::Result<()> {
    const CHUNK_SIZE: usize = 1000;

    let mut jobs = Vec::with_capacity(CHUNK_SIZE);

    let job_partitions = [&partitions.active_jobs, &partitions.inactive_jobs];

    for partition in job_partitions {
        for job_data in partition.read(tx).iter() {
            let (job_id, job_data) = job_data?;
            let job_data = job_data.value();

            jobs.push(ExportedJob {
                id: job_id.value().into(),
                created_at: Some(deserialize_systemtime(job_data.created_at).into()),
                schedule_id: job_data.schedule_id.as_ref().map(ToString::to_string),
                job_type_id: job_data.job_type_id.as_str().into(),
                target_execution_time: Some(
                    deserialize_systemtime(job_data.target_execution_time).into(),
                ),
                retry_policy: Some(
                    ora_storage::JobRetryPolicy::from(
                        deserialize!(&job_data.retry_policy).unwrap(),
                    )
                    .into(),
                ),
                timeout_policy: Some(
                    ora_storage::JobTimeoutPolicy::from(
                        deserialize!(&job_data.timeout_policy).unwrap(),
                    )
                    .into(),
                ),
                labels: job_data
                    .labels
                    .iter()
                    .map(|(k, v)| ora_proto::common::v1::JobLabel {
                        key: k.as_str().into(),
                        value: v.as_str().into(),
                    })
                    .collect(),
                marked_unschedulable_at: job_data
                    .marked_unschedulable_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                cancelled_at: job_data
                    .cancelled_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                input_payload_json: job_data.input_payload_json.as_str().into(),
                metadata_json: job_data.metadata_json.as_ref().map(|s| s.as_str().into()),
            });

            if jobs.len() >= CHUNK_SIZE {
                if snd
                    .send(Ok(SnapshotData {
                        jobs: jobs.clone(),
                        ..Default::default()
                    }))
                    .is_err()
                {
                    return Ok(());
                }

                jobs.clear();
            }
        }
    }

    if !jobs.is_empty()
        && snd
            .send(Ok(SnapshotData {
                jobs,
                ..Default::default()
            }))
            .is_err()
    {
        return Ok(());
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
fn export_executions(
    tx: &ReadTransaction,
    partitions: &Partitions,
    snd: &flume::Sender<eyre::Result<SnapshotData>>,
) -> eyre::Result<()> {
    const CHUNK_SIZE: usize = 1000;

    let mut executions = Vec::with_capacity(CHUNK_SIZE);

    let execution_partitions = [
        &partitions.pending_executions,
        &partitions.ready_executions,
        &partitions.assigned_executions,
        &partitions.running_executions,
        &partitions.succeeded_executions,
        &partitions.failed_executions,
    ];

    for partition in execution_partitions {
        for execution_data in partition.read(tx).iter() {
            let (execution_id, execution_data) = execution_data?;
            let execution_data = execution_data.value();

            executions.push(ora_proto::snapshot::v1::ExportedExecution {
                id: execution_id.value().into(),
                job_id: execution_data.job_id.into(),
                target_execution_time: Some(
                    deserialize_systemtime(execution_data.target_execution_time).into(),
                ),
                executor_id: execution_data.executor_id.as_ref().map(ToString::to_string),
                created_at: Some(deserialize_systemtime(execution_data.created_at).into()),
                ready_at: execution_data
                    .ready_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                assigned_at: execution_data
                    .assigned_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                started_at: execution_data
                    .started_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                succeeded_at: execution_data
                    .succeeded_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                failed_at: execution_data
                    .failed_at
                    .as_ref()
                    .map(|t| deserialize_systemtime(*t).into()),
                output_payload_json: execution_data
                    .output_payload_json
                    .as_ref()
                    .map(|s| s.as_str().into()),
                failure_reason: execution_data
                    .failure_reason
                    .as_ref()
                    .map(|s| s.as_str().into()),
            });

            if executions.len() >= CHUNK_SIZE {
                if snd
                    .send(Ok(SnapshotData {
                        executions: executions.clone(),
                        ..Default::default()
                    }))
                    .is_err()
                {
                    return Ok(());
                }

                executions.clear();
            }
        }
    }

    if !executions.is_empty()
        && snd
            .send(Ok(SnapshotData {
                executions,
                ..Default::default()
            }))
            .is_err()
    {
        return Ok(());
    }

    Ok(())
}
