//! A persistent storage implementation based on the fjall storage engine.

use std::{mem, time::SystemTime};

use async_trait::async_trait;
use eyre::{bail, Context};
use fjall::{PersistMode, ReadTransaction, TxKeyspace, WriteTransaction};
use models::{ExecutionData, JobData, JobTypeData, ScheduleData};
use ora_storage::IndexSet;
use ora_storage::{PendingSchedule, Storage};
use partitions::Partitions;
use tokio::task::spawn_blocking;
use typed::Raw;
use util::deserialize_systemtime;
use uuid::Uuid;

#[macro_use]
pub(crate) mod util;

mod indexes;
mod job_query;
mod models;
mod partitions;
mod schedule_query;
mod snapshot;
mod typed;

/// Configuration for the fjall storage engine.
#[must_use]
pub struct FjallStorageConfig {
    fjall_config: fjall::Config,
    durability: Option<PersistMode>,
}

impl FjallStorageConfig {
    /// Creates a new configuration for the fjall storage engine.
    pub fn new(fjall_config: fjall::Config) -> Self {
        Self {
            fjall_config,
            durability: Some(PersistMode::SyncAll),
        }
    }

    /// Sets the durability mode to use for transactions.
    pub fn transcation_durability(mut self, durability: Option<PersistMode>) -> Self {
        self.durability = durability;
        self
    }
}

/// A persistent storage implementation based on the fjall storage engine.
#[derive(Clone)]
pub struct FjallStorage {
    keyspace: TxKeyspace,
    /// The durability mode to use for transactions.
    durability: Option<PersistMode>,
    partitions: Partitions,
}

impl std::fmt::Debug for FjallStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FjallStorage").finish_non_exhaustive()
    }
}

impl FjallStorage {
    /// Creates a new fjall storage instance.
    pub fn new(config: FjallStorageConfig) -> eyre::Result<Self> {
        let keyspace = config.fjall_config.open_transactional()?;

        let partitions = Partitions::new(&keyspace)?;

        Ok(Self {
            partitions,
            keyspace,
            durability: config.durability,
        })
    }

    async fn write<F, T>(&self, f: F) -> eyre::Result<T>
    where
        F: for<'a> FnOnce(WriteTransaction<'a>, &'a Partitions) -> eyre::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let keyspace = self.keyspace.clone();
        let partitions = self.partitions.clone();
        let durability = self.durability;

        spawn_blocking(move || f(keyspace.write_tx().durability(durability), &partitions))
            .await
            .map_err(eyre::Report::from)?
    }

    async fn read<F, T>(&self, f: F) -> eyre::Result<T>
    where
        F: for<'a> FnOnce(ReadTransaction, &'a Partitions) -> eyre::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let keyspace = self.keyspace.clone();
        let partitions = self.partitions.clone();

        spawn_blocking(move || f(keyspace.read_tx(), &partitions))
            .await
            .map_err(eyre::Report::from)?
    }
}

#[async_trait]
impl Storage for FjallStorage {
    async fn job_types_added(&self, job_types: Vec<ora_storage::JobType>) -> eyre::Result<()> {
        self.write(|mut tx, partitions| {
            for mut job_type in job_types {
                let job_type_id = mem::take(&mut job_type.id);
                let job_type_data = JobTypeData::from(job_type);

                partitions
                    .job_types
                    .write(&mut tx)
                    .insert(&job_type_id, &job_type_data);
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn jobs_added(&self, jobs: Vec<ora_storage::NewJob>) -> eyre::Result<()> {
        self.write(|mut tx, partitions| {
            for job in jobs {
                let job_data = JobData::from(job);

                partitions
                    .active_jobs
                    .write(&mut tx)
                    .insert(&job_data.id, &job_data);

                for (key, value) in job_data.labels {
                    let label_key = indexes::LabelIndexKey::new(&key, &value, job_data.id);
                    partitions
                        .idx_job_labels
                        .write(&mut tx)
                        .insert(&label_key, &());
                }

                partitions
                    .idx_pending_jobs
                    .write(&mut tx)
                    .insert(&job_data.id, &());

                if let Some(schedule_id) = job_data.schedule_id {
                    let schedule_job_key =
                        indexes::ScheduleJobIndexKey::new(schedule_id, job_data.id);
                    partitions
                        .idx_schedule_jobs
                        .write(&mut tx)
                        .insert(&schedule_job_key, &());
                    partitions
                        .idx_schedule_active_job
                        .write(&mut tx)
                        .insert(&schedule_id, &job_data.id);
                    partitions
                        .idx_job_schedule
                        .write(&mut tx)
                        .insert(&job_data.id, &schedule_id);
                    partitions
                        .idx_pending_schedules
                        .write(&mut tx)
                        .remove(&schedule_id);
                }
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn jobs_cancelled(
        &self,
        job_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<Vec<ora_storage::CancelledJob>> {
        let mut job_ids: Vec<_> = job_ids.into();
        job_ids.sort();

        self.write(move |mut tx, partitions| {
            let mut cancelled_jobs = Vec::with_capacity(job_ids.len());

            for job_id in job_ids {
                if let Some(job_data) = partitions.active_jobs.write(&mut tx).get(&job_id)? {
                    let job_data = job_data.value();

                    if job_data.cancelled_at.is_some() {
                        continue;
                    }

                    let mut job_data = deserialize!(job_data)?;
                    job_data.cancelled_at = Some(timestamp);

                    partitions
                        .active_jobs
                        .write(&mut tx)
                        .insert(&job_id, &job_data);
                    job_unschedulable(partitions, &mut tx, job_id, timestamp)?;

                    let active_execution = partitions
                        .idx_job_active_execution
                        .write(&mut tx)
                        .get(&job_id)?
                        .map(|id| id.value());

                    cancelled_jobs.push(ora_storage::CancelledJob {
                        id: job_id,
                        active_execution,
                    });
                }
            }

            tx.commit()?;

            Ok(cancelled_jobs)
        })
        .await
    }

    async fn executions_added(
        &self,
        executions: Vec<ora_storage::NewExecution>,
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        self.write(move |mut tx, partitions| {
            for execution in executions {
                let Some(job) = partitions
                    .active_jobs
                    .write(&mut tx)
                    .get(&execution.job_id)?
                else {
                    continue;
                };

                let target_execution_time =
                    deserialize_systemtime(job.value().target_execution_time);

                let execution_data = ExecutionData {
                    id: execution.id,
                    job_id: execution.job_id,
                    executor_id: None,
                    created_at: timestamp,
                    ready_at: None,
                    assigned_at: None,
                    started_at: None,
                    succeeded_at: None,
                    failed_at: None,
                    output_payload_json: None,
                    failure_reason: None,
                    target_execution_time,
                };
                partitions
                    .pending_executions
                    .write(&mut tx)
                    .insert(&execution.id, &execution_data);

                partitions.idx_job_executions.write(&mut tx).insert(
                    &indexes::JobExecutionIndexKey::new(execution.job_id, execution.id),
                    &(),
                );
                partitions
                    .idx_job_active_execution
                    .write(&mut tx)
                    .insert(&execution.job_id, &execution.id);

                partitions
                    .idx_pending_jobs
                    .write(&mut tx)
                    .remove(&execution.job_id);
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn executions_ready(
        &self,
        execution_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        let mut execution_ids: Vec<_> = execution_ids.into();
        execution_ids.sort();

        self.write(move |mut tx, partitions| {
            for execution_id in execution_ids {
                if let Some(execution_data) = partitions
                    .pending_executions
                    .write(&mut tx)
                    .take(&execution_id)?
                {
                    let mut execution_data = deserialize!(execution_data.value())?;
                    execution_data.ready_at = Some(timestamp);

                    partitions
                        .ready_executions
                        .write(&mut tx)
                        .insert(&execution_id, &execution_data);
                }
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn execution_assigned(
        &self,
        execution_id: Uuid,
        executor_id: Uuid,
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        self.write(move |mut tx, partitions| {
            if let Some(execution_data) = partitions
                .ready_executions
                .write(&mut tx)
                .take(&execution_id)?
            {
                let mut execution_data = deserialize!(execution_data.value())?;
                execution_data.assigned_at = Some(timestamp);
                execution_data.executor_id = Some(executor_id);

                partitions
                    .assigned_executions
                    .write(&mut tx)
                    .insert(&execution_id, &execution_data);
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn execution_started(
        &self,
        execution_id: Uuid,
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        self.write(move |mut tx, partitions| {
            if let Some(execution_data) = partitions
                .assigned_executions
                .write(&mut tx)
                .take(&execution_id)?
            {
                let mut execution_data = deserialize!(execution_data.value())?;
                execution_data.started_at = Some(timestamp);

                partitions
                    .running_executions
                    .write(&mut tx)
                    .insert(&execution_id, &execution_data);
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn execution_succeeded(
        &self,
        execution_id: Uuid,
        timestamp: SystemTime,
        output_payload_json: String,
    ) -> eyre::Result<()> {
        self.write(move |mut tx, partitions| {
            if let Some(execution_data) = partitions
                .running_executions
                .write(&mut tx)
                .take(&execution_id)?
            {
                let mut execution_data = deserialize!(execution_data.value())?;
                execution_data.succeeded_at = Some(timestamp);
                execution_data.output_payload_json = Some(output_payload_json);

                partitions
                    .succeeded_executions
                    .write(&mut tx)
                    .insert(&execution_id, &execution_data);

                partitions
                    .idx_job_active_execution
                    .write(&mut tx)
                    .remove(&execution_data.job_id);

                job_unschedulable(partitions, &mut tx, execution_data.job_id, timestamp)?;
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn executions_failed(
        &self,
        execution_ids: &[Uuid],
        timestamp: SystemTime,
        reason: String,
        mark_job_unschedulable: bool,
    ) -> eyre::Result<()> {
        let mut execution_ids: Vec<_> = execution_ids.into();
        execution_ids.sort();

        self.write(move |mut tx, partitions| {
            // Executions may fail from any prior state,
            // so we loop through all partitions.
            let search_partitions = [
                &partitions.running_executions,
                &partitions.assigned_executions,
                &partitions.ready_executions,
                &partitions.pending_executions,
            ];

            for execution_id in execution_ids {
                'partition_search: for partition in search_partitions {
                    if let Some(execution_data) = partition.write(&mut tx).take(&execution_id)? {
                        let mut execution_data = deserialize!(execution_data.value())?;
                        execution_data.failed_at = Some(timestamp);
                        execution_data.failure_reason = Some(reason.clone());

                        partitions
                            .failed_executions
                            .write(&mut tx)
                            .insert(&execution_id, &execution_data);

                        partitions
                            .idx_job_active_execution
                            .write(&mut tx)
                            .remove(&execution_data.job_id);

                        if mark_job_unschedulable {
                            job_unschedulable(
                                partitions,
                                &mut tx,
                                execution_data.job_id,
                                timestamp,
                            )?;
                        } else {
                            partitions
                                .idx_pending_jobs
                                .write(&mut tx)
                                .insert(&execution_data.job_id, &());
                        }

                        break 'partition_search;
                    }
                }
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn orphan_execution_ids(&self, executor_ids: &[Uuid]) -> eyre::Result<Vec<Uuid>> {
        let executor_ids: IndexSet<_> = executor_ids.iter().copied().collect();

        self.read(move |tx, partitions| {
            let mut orphaned_execution_ids = Vec::new();

            let search_partitions = [
                &partitions.assigned_executions,
                &partitions.running_executions,
            ];

            for partition in search_partitions {
                for execution in partition.read(&tx).iter() {
                    let (_, execution) = execution?;
                    let execution = execution.value();

                    if !executor_ids.contains(&execution.executor_id.unwrap()) {
                        orphaned_execution_ids.push(execution.id);
                    }
                }
            }

            Ok(orphaned_execution_ids)
        })
        .await
    }

    async fn jobs_unschedulable(
        &self,
        job_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        let mut job_ids: Vec<_> = job_ids.into();
        job_ids.sort();

        self.write(move |mut tx, partitions| {
            for job_id in job_ids {
                job_unschedulable(partitions, &mut tx, job_id, timestamp)?;
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn pending_executions(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::PendingExecution>> {
        self.read(move |tx, partitions| {
            let mut pending_executions = Vec::new();

            if let Some(after) = after {
                for pending_execution in partitions.pending_executions.read(&tx).range(after..) {
                    let (_, pending_execution) = pending_execution?;
                    let pending_execution = pending_execution.value();

                    if pending_execution.id <= after {
                        continue;
                    }

                    pending_executions.push(ora_storage::PendingExecution {
                        id: pending_execution.id,
                        target_execution_time: deserialize_systemtime(
                            pending_execution.target_execution_time,
                        ),
                    });
                }
            } else {
                for pending_execution in partitions.pending_executions.read(&tx).iter() {
                    let (_, pending_execution) = pending_execution?;
                    let pending_execution = pending_execution.value();

                    pending_executions.push(ora_storage::PendingExecution {
                        id: pending_execution.id,
                        target_execution_time: deserialize_systemtime(
                            pending_execution.target_execution_time,
                        ),
                    });
                }
            }

            Ok(pending_executions)
        })
        .await
    }

    async fn ready_executions(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::ReadyExecution>> {
        self.read(move |tx, partitions| {
            const BATCH_SIZE: usize = 10_000;

            let mut ready_executions = Vec::with_capacity(BATCH_SIZE);

            let mut add_execution = |ready_execution: Raw<ExecutionData>| -> eyre::Result<()> {
                let ready_execution = ready_execution.value();

                let job_id = ready_execution.job_id;
                let job_data = if let Some(job_data) =
                    partitions.active_jobs.read(&tx).get(&job_id)?
                {
                    job_data
                } else if let Some(job_data) = partitions.inactive_jobs.read(&tx).get(&job_id)? {
                    job_data
                } else {
                    tracing::error!(%job_id, "found ready execution for missing job");
                    return Ok(());
                };
                let job_data = job_data.value();

                let job_execution_count = partitions
                    .idx_job_executions
                    .read(&tx)
                    .prefix(&indexes::JobExecutionIndexKey::new_prefix(job_id))
                    .count();

                ready_executions.push(ora_storage::ReadyExecution {
                    id: ready_execution.id,
                    job_id: ready_execution.job_id,
                    input_payload_json: job_data.input_payload_json.as_str().into(),
                    attempt_number: u64::try_from(job_execution_count)?,
                    job_type_id: job_data.job_type_id.as_str().into(),
                    target_execution_time: deserialize_systemtime(
                        ready_execution.target_execution_time,
                    ),
                    timeout_policy: deserialize_archived!(&job_data.timeout_policy)
                        .unwrap()
                        .into(),
                });

                Ok(())
            };

            if let Some(after) = after {
                for (i, ready_execution) in partitions
                    .ready_executions
                    .read(&tx)
                    .range(after..)
                    .enumerate()
                {
                    let (execution_id, execution_data) = ready_execution?;
                    if execution_id.value() <= after {
                        continue;
                    }

                    add_execution(execution_data)?;

                    if i + 1 == BATCH_SIZE {
                        break;
                    }
                }
            } else {
                for (i, ready_execution) in partitions.ready_executions.read(&tx).iter().enumerate()
                {
                    add_execution(ready_execution?.1)?;

                    if i + 1 == BATCH_SIZE {
                        break;
                    }
                }
            }

            Ok(ready_executions)
        })
        .await
    }

    async fn pending_jobs(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::PendingJob>> {
        self.read(move |tx, partitions| {
            const BATCH_SIZE: usize = 10_000;

            let mut pending_jobs = Vec::with_capacity(BATCH_SIZE);

            for (i, pending_job) in partitions.idx_pending_jobs.read(&tx).iter().enumerate() {
                let (job_id, _) = pending_job?;
                let job_id = job_id.value();

                // FIXME(perf): use a range query instead of filtering
                if let Some(after) = after {
                    if job_id <= after {
                        continue;
                    }
                }

                let Some(job_data) = partitions.active_jobs.read(&tx).get(&job_id)? else {
                    continue;
                };
                let job_data = job_data.value();

                let execution_count = partitions
                    .idx_job_executions
                    .read(&tx)
                    .prefix(&indexes::JobExecutionIndexKey::new_prefix(job_id))
                    .count();

                pending_jobs.push(ora_storage::PendingJob {
                    id: job_id,
                    target_execution_time: deserialize_systemtime(job_data.target_execution_time),
                    execution_count: u64::try_from(execution_count)?,
                    retry_policy: deserialize_archived!(&job_data.retry_policy)
                        .unwrap()
                        .into(),
                    timeout_policy: deserialize_archived!(&job_data.timeout_policy)
                        .unwrap()
                        .into(),
                });

                if i + 1 == BATCH_SIZE {
                    break;
                }
            }

            Ok(pending_jobs)
        })
        .await
    }

    async fn query_jobs(
        &self,
        cursor: Option<String>,
        limit: usize,
        order: ora_storage::JobQueryOrder,
        filters: ora_storage::JobQueryFilters,
    ) -> eyre::Result<ora_storage::JobQueryResult> {
        self.read(move |tx, partitions| {
            let cursor: Option<job_query::Cursor> = match cursor {
                Some(cursor) => serde_json::from_str(&cursor).wrap_err("invalid cursor")?,
                None => None,
            };

            job_query::query_jobs(&tx, partitions, cursor, limit, filters, order)
        })
        .await
    }

    async fn query_job_ids(
        &self,
        filters: ora_storage::JobQueryFilters,
    ) -> eyre::Result<Vec<Uuid>> {
        self.read(move |tx, partitions| job_query::query_job_ids(&tx, partitions, filters))
            .await
    }

    async fn count_jobs(&self, filters: ora_storage::JobQueryFilters) -> eyre::Result<u64> {
        self.read(move |tx, partitions| job_query::count_jobs(&tx, partitions, filters))
            .await
    }

    async fn query_job_types(&self) -> eyre::Result<Vec<ora_storage::JobType>> {
        self.read(|tx, partitions| {
            let mut job_types = Vec::new();

            for job_type in partitions.job_types.read(&tx).iter() {
                let (job_type_id, job_type) = job_type?;
                let job_type = job_type.value();

                job_types.push(ora_storage::JobType {
                    id: job_type_id.value().to_string(),
                    name: job_type.name.as_str().into(),
                    description: job_type.description.as_str().into(),
                    input_schema_json: job_type
                        .input_schema_json
                        .as_ref()
                        .map(|s| s.as_str().into()),
                    output_schema_json: job_type
                        .output_schema_json
                        .as_ref()
                        .map(|s| s.as_str().into()),
                });
            }

            Ok(job_types)
        })
        .await
    }

    async fn delete_jobs(
        &self,
        mut filters: ora_storage::JobQueryFilters,
    ) -> eyre::Result<Vec<Uuid>> {
        // Deleting active jobs may cause all sorts of issues,
        // so we don't allow it.
        //
        // The server itself should also set this filter, we're
        // just being extra cautious here.
        filters.active = Some(false);

        let ids_to_delete = self
            .read(move |tx, partitions| job_query::query_job_ids(&tx, partitions, filters))
            .await?;

        self.write(move |mut tx, partitions| {
            let mut deleted_jobs = Vec::with_capacity(ids_to_delete.len());

            for job_id in ids_to_delete {
                if delete_job(partitions, &mut tx, job_id)? {
                    deleted_jobs.push(job_id);
                }
            }

            tx.commit()?;

            Ok(deleted_jobs)
        })
        .await
    }

    async fn schedules_added(&self, schedules: Vec<ora_storage::NewSchedule>) -> eyre::Result<()> {
        self.write(move |mut tx, partitions| {
            for schedule in schedules {
                let schedule_data = ScheduleData::from(schedule);

                partitions
                    .active_schedules
                    .write(&mut tx)
                    .insert(&schedule_data.id, &schedule_data);

                for (key, value) in schedule_data.labels {
                    let label_key = indexes::LabelIndexKey::new(&key, &value, schedule_data.id);
                    partitions
                        .idx_schedule_labels
                        .write(&mut tx)
                        .insert(&label_key, &());
                }

                partitions
                    .idx_pending_schedules
                    .write(&mut tx)
                    .insert(&schedule_data.id, &());
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn schedules_cancelled(
        &self,
        schedule_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<Vec<ora_storage::CancelledSchedule>> {
        let mut schedule_ids: Vec<_> = schedule_ids.into();
        schedule_ids.sort();

        self.write(move |mut tx, partitions| {
            let mut cancelled_schedules = Vec::with_capacity(schedule_ids.len());

            for schedule_id in schedule_ids {
                if let Some(schedule_data) = partitions
                    .active_schedules
                    .write(&mut tx)
                    .get(&schedule_id)?
                {
                    let schedule_data = schedule_data.value();

                    if schedule_data.cancelled_at.is_some() {
                        continue;
                    }

                    let mut schedule_data = deserialize!(schedule_data)?;
                    schedule_data.cancelled_at = Some(timestamp);

                    partitions
                        .active_schedules
                        .write(&mut tx)
                        .insert(&schedule_id, &schedule_data);
                    schedule_unschedulable(partitions, &mut tx, schedule_id, timestamp)?;

                    cancelled_schedules.push(ora_storage::CancelledSchedule { id: schedule_id });
                }
            }

            tx.commit()?;

            Ok(cancelled_schedules)
        })
        .await
    }

    async fn schedules_unschedulable(
        &self,
        schedule_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        let mut schedule_ids: Vec<_> = schedule_ids.into();
        schedule_ids.sort();

        self.write(move |mut tx, partitions| {
            for schedule_id in schedule_ids {
                schedule_unschedulable(partitions, &mut tx, schedule_id, timestamp)?;
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    async fn pending_schedules(&self, after: Option<Uuid>) -> eyre::Result<Vec<PendingSchedule>> {
        self.read(move |tx, partitions| {
            const BATCH_SIZE: usize = 10_000;

            let mut pending_schedules = Vec::with_capacity(BATCH_SIZE);

            for (i, pending_schedule) in partitions
                .idx_pending_schedules
                .read(&tx)
                .iter()
                .enumerate()
            {
                let (schedule_id, _) = pending_schedule?;
                let schedule_id = schedule_id.value();

                // FIXME(perf): use a range query instead of filtering
                if let Some(after) = after {
                    if schedule_id <= after {
                        continue;
                    }
                }

                let Some(schedule_data) =
                    partitions.active_schedules.read(&tx).get(&schedule_id)?
                else {
                    continue;
                };
                let schedule_data = schedule_data.value();

                let last_schedule_job_id = partitions
                    .idx_schedule_jobs
                    .read(&tx)
                    .prefix(&indexes::ScheduleJobIndexKey::new_prefix(schedule_id))
                    .last()
                    .transpose()?
                    .map(|(job_id, _)| job_id.value().job_id.unwrap());

                let last_target_execution_time = match last_schedule_job_id {
                    Some(job_id) => {
                        let job_data = partitions.inactive_jobs.read(&tx).get(&job_id)?;
                        let job_data = match job_data {
                            Some(job_data) => Some(job_data),
                            None => partitions.active_jobs.read(&tx).get(&job_id)?,
                        };

                        match job_data {
                            Some(job_data) => {
                                let job_data = job_data.value();

                                Some(job_data.target_execution_time)
                            }
                            None => None,
                        }
                    }
                    None => None,
                };

                pending_schedules.push(ora_storage::PendingSchedule {
                    id: schedule_id,
                    job_timing_policy: deserialize!(&schedule_data.job_timing_policy)?.into(),
                    job_creation_policy: deserialize!(&schedule_data.job_creation_policy)?.into(),
                    last_target_execution_time: last_target_execution_time
                        .map(deserialize_systemtime),
                    time_range: match schedule_data.time_range.as_ref() {
                        Some(time_range) => Some(deserialize!(time_range)?.into()),
                        None => None,
                    },
                });

                if i + 1 == BATCH_SIZE {
                    break;
                }
            }

            Ok(pending_schedules)
        })
        .await
    }

    async fn query_schedules(
        &self,
        cursor: Option<String>,
        limit: usize,
        filters: ora_storage::ScheduleQueryFilters,
        order: ora_storage::ScheduleQueryOrder,
    ) -> eyre::Result<ora_storage::ScheduleQueryResult> {
        self.read(move |tx, partitions| {
            let cursor: Option<schedule_query::Cursor> = match cursor {
                Some(cursor) => serde_json::from_str(&cursor).wrap_err("invalid cursor")?,
                None => None,
            };

            schedule_query::query_schedules(&tx, partitions, cursor, limit, filters, order)
        })
        .await
    }

    async fn query_schedule_ids(
        &self,
        filters: ora_storage::ScheduleQueryFilters,
    ) -> eyre::Result<Vec<Uuid>> {
        self.read(move |tx, partitions| {
            schedule_query::query_schedule_ids(&tx, partitions, filters)
        })
        .await
    }

    async fn count_schedules(
        &self,
        filters: ora_storage::ScheduleQueryFilters,
    ) -> eyre::Result<u64> {
        self.read(move |tx, partitions| schedule_query::count_schedules(&tx, partitions, filters))
            .await
    }

    async fn delete_schedules(
        &self,
        mut filters: ora_storage::ScheduleQueryFilters,
    ) -> eyre::Result<Vec<Uuid>> {
        // Deleting active schedules may cause all sorts of issues,
        // so we don't allow it.
        //
        // The server itself should also set this filter, we're
        // just being extra cautious here.
        filters.active = Some(false);

        let ids_to_delete = self
            .read(move |tx, partitions| {
                schedule_query::query_schedule_ids(&tx, partitions, filters)
            })
            .await?;

        self.write(move |mut tx, partitions| {
            let mut deleted_schedules = Vec::with_capacity(ids_to_delete.len());

            for schedule_id in ids_to_delete {
                // Delete the schedule itself.
                let Some(schedule_data) = partitions
                    .inactive_schedules
                    .write(&mut tx)
                    .take(&schedule_id)?
                else {
                    continue;
                };

                deleted_schedules.push(schedule_id);

                let schedule_data = schedule_data.value();

                // Remove the schedule from indexes.
                for (key, value) in schedule_data.labels.iter() {
                    let label_key = indexes::LabelIndexKey::new(key, value, schedule_id);
                    partitions
                        .idx_schedule_labels
                        .write(&mut tx)
                        .remove(&label_key);
                }

                // Also cascade delete all jobs associated with the schedule.
                let mut job_ids = Vec::new();

                for schedule_job in partitions
                    .idx_schedule_jobs
                    .write(&mut tx)
                    .prefix(&indexes::ScheduleJobIndexKey::new_prefix(schedule_id))
                {
                    let (job_id, _) = schedule_job?;
                    let job_id = job_id.value().job_id.unwrap();

                    job_ids.push(job_id);
                }

                for job_id in job_ids {
                    delete_job(partitions, &mut tx, job_id)?;
                }
            }

            tx.commit()?;

            Ok(deleted_schedules)
        })
        .await
    }

    async fn job_added_conditionally(
        &self,
        _job: ora_storage::NewJob,
        _filters: ora_storage::JobQueryFilters,
    ) -> eyre::Result<ora_storage::ConditionalJobResult> {
        bail!("not supported")
    }

    async fn schedule_added_conditionally(
        &self,
        _schedule: ora_storage::NewSchedule,
        _filters: ora_storage::ScheduleQueryFilters,
    ) -> eyre::Result<ora_storage::ConditionalScheduleResult> {
        bail!("not supported")
    }
}

fn delete_job<'a>(
    partitions: &'a Partitions,
    tx: &mut WriteTransaction<'a>,
    job_id: Uuid,
) -> Result<bool, eyre::Error> {
    let Some(job_data) = partitions.inactive_jobs.write(tx).take(&job_id)? else {
        return Ok(false);
    };
    let job_data = job_data.value();
    let mut execution_ids = Vec::new();
    for job_execution in partitions
        .idx_job_executions
        .write(tx)
        .prefix(&indexes::JobExecutionIndexKey::new_prefix(job_id))
    {
        let (job_execution_id, _) = job_execution?;
        let job_execution_id = job_execution_id.value().execution_id.unwrap();

        execution_ids.push(job_execution_id);
    }
    for job_execution_id in execution_ids {
        partitions
            .succeeded_executions
            .write(tx)
            .remove(&job_execution_id);
        partitions
            .failed_executions
            .write(tx)
            .remove(&job_execution_id);

        partitions
            .idx_job_executions
            .write(tx)
            .remove(&indexes::JobExecutionIndexKey::new(
                job_id,
                job_execution_id,
            ));
    }
    for (key, value) in job_data.labels.iter() {
        let label_key = indexes::LabelIndexKey::new(key, value, job_id);
        partitions.idx_job_labels.write(tx).remove(&label_key);
    }

    if let Some(schedule_id) = partitions.idx_job_schedule.write(tx).get(&job_id)? {
        partitions
            .idx_schedule_jobs
            .write(tx)
            .remove(&indexes::ScheduleJobIndexKey::new(
                schedule_id.value(),
                job_id,
            ));
        partitions.idx_job_schedule.write(tx).remove(&job_id);
    }

    Ok(true)
}

fn job_unschedulable<'a>(
    partitions: &'a Partitions,
    tx: &mut WriteTransaction<'a>,
    job_id: Uuid,
    timestamp: SystemTime,
) -> Result<(), eyre::Error> {
    if let Some(job_data) = partitions.active_jobs.write(tx).take(&job_id)? {
        let mut job_data = deserialize!(job_data.value())?;

        if job_data.marked_unschedulable_at.is_none() {
            job_data.marked_unschedulable_at = Some(timestamp);
        }

        if partitions
            .idx_job_active_execution
            .write(tx)
            .contains_key(&job_id)?
        {
            // We cannot mark the job inactive while it has active executions,
            // so we keep it in the active state but mark it unschedulable.
            partitions.active_jobs.write(tx).insert(&job_id, &job_data);
        } else {
            partitions
                .inactive_jobs
                .write(tx)
                .insert(&job_id, &job_data);

            if let Some(schedule_id) = job_data.schedule_id {
                partitions
                    .idx_schedule_active_job
                    .write(tx)
                    .remove(&schedule_id);

                if let Some(schedule_data) =
                    partitions.active_schedules.write(tx).get(&schedule_id)?
                {
                    let schedule_data = schedule_data.value();

                    // Last job of the schedule is done, unless the schedule
                    // was cancelled, we mark it as pending again.
                    if schedule_data.cancelled_at.is_none() {
                        partitions
                            .idx_pending_schedules
                            .write(tx)
                            .insert(&schedule_id, &());
                    }
                }
            }
        }

        partitions.idx_pending_jobs.write(tx).remove(&job_id);
    }
    Ok(())
}

fn schedule_unschedulable<'a>(
    partitions: &'a Partitions,
    tx: &mut WriteTransaction<'a>,
    schedule_id: Uuid,
    timestamp: SystemTime,
) -> Result<(), eyre::Error> {
    if let Some(schedule_data) = partitions.active_schedules.write(tx).take(&schedule_id)? {
        let mut schedule_data = deserialize!(schedule_data.value())?;

        if schedule_data.marked_unschedulable_at.is_none() {
            schedule_data.marked_unschedulable_at = Some(timestamp);
        }

        if partitions
            .idx_schedule_active_job
            .write(tx)
            .contains_key(&schedule_id)?
        {
            // We cannot mark the schedule inactive while it has active jobs,
            // so we keep it in the active state but mark it unschedulable.
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

        partitions
            .idx_pending_schedules
            .write(tx)
            .remove(&schedule_id);
    }
    Ok(())
}
