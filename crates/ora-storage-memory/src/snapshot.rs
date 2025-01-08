use async_trait::async_trait;
use futures::{stream::BoxStream, StreamExt, TryStreamExt};
use ora_proto::snapshot::v1::{self, SnapshotData};
use tokio::task::spawn_blocking;

use ora_storage::{JobType, StorageSnapshot};

use super::{Execution, Job, MemoryStorage, Schedule};

#[async_trait]
impl StorageSnapshot for MemoryStorage {
    fn export_snapshot(&self) -> BoxStream<'static, eyre::Result<SnapshotData>> {
        let this = self.clone();

        // Make sure that we don't block the runtime.
        let data = spawn_blocking(move || {
            let Self {
                job_types,
                schedulable_jobs: active_jobs,
                unschedulable_jobs: inactive_jobs,
                pending_executions,
                ready_executions,
                assigned_executions,
                started_executions,
                succeeded_executions,
                failed_executions,
                schedulable_schedules: active_schedules,
                unschedulable_schedules: inactive_schedules,
            } = this;

            let job_types = job_types.read();
            let active_jobs = active_jobs.read();
            let inactive_jobs = inactive_jobs.read();
            let pending_executions = pending_executions.read();
            let ready_executions = ready_executions.read();
            let assigned_executions = assigned_executions.read();
            let started_executions = started_executions.read();
            let succeeded_executions = succeeded_executions.read();
            let failed_executions = failed_executions.read();
            let active_schedules = active_schedules.read();
            let inactive_schedules = inactive_schedules.read();

            SnapshotData {
                jobs: active_jobs
                    .values()
                    .cloned()
                    .map(Into::into)
                    .chain(inactive_jobs.values().cloned().map(Into::into))
                    .collect(),
                executions: pending_executions
                    .values()
                    .cloned()
                    .map(Into::into)
                    .chain(ready_executions.values().cloned().map(Into::into))
                    .chain(assigned_executions.values().cloned().map(Into::into))
                    .chain(started_executions.values().cloned().map(Into::into))
                    .chain(succeeded_executions.values().cloned().map(Into::into))
                    .chain(failed_executions.values().cloned().map(Into::into))
                    .collect(),
                schedules: active_schedules
                    .values()
                    .cloned()
                    .map(Into::into)
                    .chain(inactive_schedules.values().cloned().map(Into::into))
                    .collect(),
                job_types: job_types.values().cloned().map(Into::into).collect(),
            }
        });

        futures::stream::once(async move { data.await.map_err(Into::into) }).boxed()
    }

    async fn import_snapshot(
        &self,
        mut snapshot: BoxStream<'static, eyre::Result<SnapshotData>>,
    ) -> eyre::Result<()> {
        while let Some(batch) = snapshot.try_next().await? {
            let this = self.clone();
            spawn_blocking(|| {
                let Self {
                    job_types,
                    schedulable_jobs: active_jobs,
                    unschedulable_jobs: inactive_jobs,
                    pending_executions,
                    ready_executions,
                    assigned_executions,
                    started_executions,
                    succeeded_executions,
                    failed_executions,
                    schedulable_schedules: active_schedules,
                    unschedulable_schedules: inactive_schedules,
                } = this;

                let mut job_types = job_types.write();
                let mut active_jobs = active_jobs.write();
                let mut inactive_jobs = inactive_jobs.write();
                let mut pending_executions = pending_executions.write();
                let mut ready_executions = ready_executions.write();
                let mut assigned_executions = assigned_executions.write();
                let mut started_executions = started_executions.write();
                let mut succeeded_executions = succeeded_executions.write();
                let mut failed_executions = failed_executions.write();
                let mut active_schedules = active_schedules.write();
                let mut inactive_schedules = inactive_schedules.write();

                for job in batch.jobs {
                    let job = Job::from(job);
                    if job.marked_unschedulable_at.is_some() {
                        inactive_jobs.insert(job.id, job);
                    } else {
                        active_jobs.insert(job.id, job);
                    }
                }

                for execution in batch.executions {
                    let execution = Execution::from(execution);
                    if execution.succeeded_at.is_some() {
                        succeeded_executions.insert(execution.id, execution);
                    } else if execution.failed_at.is_some() {
                        failed_executions.insert(execution.id, execution);
                    } else if execution.started_at.is_some() {
                        started_executions.insert(execution.id, execution);
                    } else if execution.assigned_at.is_some() {
                        assigned_executions.insert(execution.id, execution);
                    } else if execution.ready_at.is_some() {
                        ready_executions.insert(execution.id, execution);
                    } else {
                        pending_executions.insert(execution.id, execution);
                    }
                }

                for schedule in batch.schedules {
                    let schedule = Schedule::from(schedule);
                    if schedule.marked_unschedulable_at.is_some() {
                        inactive_schedules.insert(schedule.id, schedule);
                    } else {
                        active_schedules.insert(schedule.id, schedule);
                    }
                }

                for job_type in batch.job_types {
                    let job_type = JobType::from(job_type);
                    job_types.insert(job_type.id.clone(), job_type);
                }
            })
            .await?;
        }

        Ok(())
    }
}

impl From<Job> for v1::ExportedJob {
    fn from(job: Job) -> Self {
        v1::ExportedJob {
            id: job.id.into(),
            schedule_id: job.schedule_id.map(Into::into),
            created_at: Some(job.created_at.into()),
            job_type_id: job.job_type_id,
            target_execution_time: Some(job.target_execution_time.into()),
            retry_policy: Some(job.retry_policy.into()),
            timeout_policy: Some(job.timeout_policy.into()),
            labels: job
                .labels
                .into_iter()
                .map(|(key, value)| ora_proto::common::v1::JobLabel { key, value })
                .collect(),
            marked_unschedulable_at: job.marked_unschedulable_at.map(Into::into),
            cancelled_at: job.cancelled_at.map(Into::into),
            input_payload_json: job.input_payload_json,
            metadata_json: job.metadata_json,
        }
    }
}

impl From<v1::ExportedJob> for Job {
    fn from(job: v1::ExportedJob) -> Self {
        Self {
            id: job.id.parse().unwrap(),
            schedule_id: job.schedule_id.map(|t| t.parse().unwrap()),
            created_at: job.created_at.unwrap().try_into().unwrap(),
            job_type_id: job.job_type_id,
            target_execution_time: job.target_execution_time.unwrap().try_into().unwrap(),
            retry_policy: job.retry_policy.unwrap().into(),
            timeout_policy: job.timeout_policy.unwrap().into(),
            labels: job
                .labels
                .into_iter()
                .map(|label| (label.key, label.value))
                .collect(),
            marked_unschedulable_at: job.marked_unschedulable_at.map(|t| t.try_into().unwrap()),
            cancelled_at: job.cancelled_at.map(|t| t.try_into().unwrap()),
            input_payload_json: job.input_payload_json,
            metadata_json: job.metadata_json,
        }
    }
}

impl From<Execution> for v1::ExportedExecution {
    fn from(execution: Execution) -> Self {
        v1::ExportedExecution {
            id: execution.id.into(),
            job_id: execution.job_id.into(),
            target_execution_time: Some(execution.target_execution_time.into()),
            executor_id: execution.executor_id.map(Into::into),
            created_at: Some(execution.created_at.into()),
            ready_at: execution.ready_at.map(Into::into),
            assigned_at: execution.assigned_at.map(Into::into),
            started_at: execution.started_at.map(Into::into),
            succeeded_at: execution.succeeded_at.map(Into::into),
            failed_at: execution.failed_at.map(Into::into),
            output_payload_json: execution.output_payload_json,
            failure_reason: execution.failure_reason,
        }
    }
}

impl From<v1::ExportedExecution> for Execution {
    fn from(execution: v1::ExportedExecution) -> Self {
        Self {
            id: execution.id.parse().unwrap(),
            job_id: execution.job_id.parse().unwrap(),
            target_execution_time: execution.target_execution_time.unwrap().try_into().unwrap(),
            executor_id: execution.executor_id.map(|t| t.parse().unwrap()),
            created_at: execution.created_at.unwrap().try_into().unwrap(),
            ready_at: execution.ready_at.map(|t| t.try_into().unwrap()),
            assigned_at: execution.assigned_at.map(|t| t.try_into().unwrap()),
            started_at: execution.started_at.map(|t| t.try_into().unwrap()),
            succeeded_at: execution.succeeded_at.map(|t| t.try_into().unwrap()),
            failed_at: execution.failed_at.map(|t| t.try_into().unwrap()),
            output_payload_json: execution.output_payload_json,
            failure_reason: execution.failure_reason,
        }
    }
}

impl From<Schedule> for v1::ExportedSchedule {
    fn from(schedule: Schedule) -> Self {
        v1::ExportedSchedule {
            id: schedule.id.into(),
            created_at: Some(schedule.created_at.into()),
            job_type_id: schedule.job_type_id,
            labels: schedule
                .labels
                .into_iter()
                .map(|(key, value)| ora_proto::common::v1::ScheduleLabel { key, value })
                .collect(),
            marked_unschedulable_at: schedule.marked_unschedulable_at.map(Into::into),
            cancelled_at: schedule.cancelled_at.map(Into::into),
            job_timing_policy: Some(schedule.job_timing_policy.into()),
            job_creation_policy: Some(schedule.job_creation_policy.into()),
            time_range: schedule.time_range.map(Into::into),
            metadata_json: schedule.metadata_json,
        }
    }
}

impl From<v1::ExportedSchedule> for Schedule {
    fn from(schedule: v1::ExportedSchedule) -> Self {
        Self {
            id: schedule.id.parse().unwrap(),
            created_at: schedule.created_at.unwrap().try_into().unwrap(),
            job_type_id: schedule.job_type_id,
            labels: schedule
                .labels
                .into_iter()
                .map(|label| (label.key, label.value))
                .collect(),
            marked_unschedulable_at: schedule
                .marked_unschedulable_at
                .map(|t| t.try_into().unwrap()),
            cancelled_at: schedule.cancelled_at.map(|t| t.try_into().unwrap()),
            job_timing_policy: schedule.job_timing_policy.unwrap().try_into().unwrap(),
            job_creation_policy: schedule.job_creation_policy.unwrap().try_into().unwrap(),
            time_range: schedule.time_range.map(|t| t.try_into().unwrap()),
            metadata_json: schedule.metadata_json,
        }
    }
}
