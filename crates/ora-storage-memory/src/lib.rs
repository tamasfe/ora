//! A simple in-memory storage backend with no persistence.

use std::{sync::Arc, time::SystemTime};

use async_trait::async_trait;
use eyre::{bail, Context};
use indexmap::IndexSet;
use parking_lot::RwLock;
use uuid::Uuid;

use ora_storage::{
    CancelledJob, CancelledSchedule, ExecutionDetails, IndexMap, JobExecutionStatus,
    JobQueryFilters, JobQueryOrder, JobQueryResult, JobRetryPolicy, JobTimeoutPolicy, JobType,
    NewExecution, NewJob, NewSchedule, PendingExecution, PendingJob, PendingSchedule,
    ReadyExecution, ScheduleJobCreationPolicy, ScheduleJobTimingPolicy, ScheduleQueryFilters,
    ScheduleQueryOrder, ScheduleQueryResult, ScheduleTimeRange, Storage,
};

mod job_query;
mod schedule_query;
mod snapshot;

/// A storage backend that holds all data in memory.
///
/// It serves as a reference implementation for storage backends
/// as well as a test platform for the server itself.
/// It is optimized for simplicity and readability, and is not
/// intended for production use.
///
/// It may be used for small-scale production applications
/// where persistence is not required and the amount of data
/// is small enough to fit in memory.
//
// Indexes and other helper data structures are omitted
// on purpose to keep the implementation simple.
//
// The implementation also contains more safety checks and
// assertions that test the server itself.
#[derive(Debug, Default, Clone)]
#[must_use]
pub struct MemoryStorage {
    job_types: Arc<RwLock<IndexMap<String, JobType>>>,

    // Jobs are partitioned by whether they are schedulable or unschedulable.
    schedulable_jobs: Arc<RwLock<IndexMap<Uuid, Job>>>,
    unschedulable_jobs: Arc<RwLock<IndexMap<Uuid, Job>>>,

    // Executions are partitioned by the phase they are in.
    pending_executions: Arc<RwLock<IndexMap<Uuid, Execution>>>,
    ready_executions: Arc<RwLock<IndexMap<Uuid, Execution>>>,
    assigned_executions: Arc<RwLock<IndexMap<Uuid, Execution>>>,
    started_executions: Arc<RwLock<IndexMap<Uuid, Execution>>>,
    succeeded_executions: Arc<RwLock<IndexMap<Uuid, Execution>>>,
    failed_executions: Arc<RwLock<IndexMap<Uuid, Execution>>>,

    // Schedules are partitioned by whether they are schedulable or unschedulable.
    schedulable_schedules: Arc<RwLock<IndexMap<Uuid, Schedule>>>,
    unschedulable_schedules: Arc<RwLock<IndexMap<Uuid, Schedule>>>,
}

impl MemoryStorage {
    /// Create a new in-memory storage backend.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn job_types_added(&self, job_types: Vec<JobType>) -> eyre::Result<()> {
        self.job_types.write().extend(
            job_types
                .iter()
                .map(|job_type| (job_type.id.clone(), job_type.clone())),
        );

        Ok(())
    }

    async fn jobs_added(&self, new_jobs: Vec<NewJob>) -> eyre::Result<()> {
        for new_job in new_jobs {
            let mut jobs = self.schedulable_jobs.write();

            if jobs.contains_key(&new_job.id) {
                bail!("job with ID {} already exists", new_job.id);
            }

            jobs.insert(new_job.id, Job::from(new_job));
        }

        Ok(())
    }

    async fn jobs_cancelled(
        &self,
        job_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<Vec<CancelledJob>> {
        let mut cancelled_jobs = Vec::with_capacity(job_ids.len());

        for job_id in job_ids {
            let active_job = self.schedulable_jobs.write().swap_remove(job_id);

            if let Some(mut job) = active_job {
                debug_assert!(job.cancelled_at.is_none());
                job.cancelled_at = Some(timestamp);

                let active_execution = self
                    .pending_executions
                    .read()
                    .iter()
                    .find_map(|(_, execution)| {
                        if &execution.job_id == job_id {
                            Some(execution.id)
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        self.ready_executions
                            .read()
                            .iter()
                            .find_map(|(_, execution)| {
                                if &execution.job_id == job_id {
                                    Some(execution.id)
                                } else {
                                    None
                                }
                            })
                    })
                    .or_else(|| {
                        self.assigned_executions
                            .read()
                            .iter()
                            .find_map(|(_, execution)| {
                                if &execution.job_id == job_id {
                                    Some(execution.id)
                                } else {
                                    None
                                }
                            })
                    })
                    .or_else(|| {
                        self.started_executions
                            .read()
                            .iter()
                            .find_map(|(_, execution)| {
                                if &execution.job_id == job_id {
                                    Some(execution.id)
                                } else {
                                    None
                                }
                            })
                    });

                if active_execution.is_some() {
                    job.marked_unschedulable_at = Some(timestamp);
                    self.unschedulable_jobs.write().insert(*job_id, job);
                } else {
                    self.schedulable_jobs.write().insert(*job_id, job);
                }

                cancelled_jobs.push(CancelledJob {
                    id: *job_id,
                    active_execution,
                });
            }
        }

        Ok(cancelled_jobs)
    }

    async fn executions_added(
        &self,
        executions: Vec<NewExecution>,
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        for execution in executions {
            let mut pending_executions = self.pending_executions.write();
            if pending_executions.contains_key(&execution.id) {
                bail!("execution with ID {} already exists", execution.id);
            }

            pending_executions.insert(
                execution.id,
                Execution {
                    id: execution.id,
                    job_id: execution.job_id,
                    target_execution_time: execution.target_execution_time,
                    created_at: timestamp,
                    executor_id: None,
                    ready_at: None,
                    assigned_at: None,
                    started_at: None,
                    succeeded_at: None,
                    failed_at: None,
                    output_payload_json: None,
                    failure_reason: None,
                },
            );
        }

        Ok(())
    }

    async fn executions_ready(
        &self,
        execution_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        for execution_id in execution_ids {
            let execution = self.pending_executions.write().swap_remove(execution_id);

            if let Some(mut execution) = execution {
                debug_assert!(execution.ready_at.is_none());
                execution.ready_at = Some(timestamp);
                self.ready_executions
                    .write()
                    .insert(*execution_id, execution);
            }
        }

        Ok(())
    }

    async fn execution_assigned(
        &self,
        execution_id: Uuid,
        executor_id: Uuid,
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        let execution = self.ready_executions.write().swap_remove(&execution_id);

        if let Some(mut execution) = execution {
            debug_assert!(execution.assigned_at.is_none());
            execution.assigned_at = Some(timestamp);
            execution.executor_id = Some(executor_id);

            self.assigned_executions
                .write()
                .insert(execution_id, execution);
        }

        Ok(())
    }

    async fn execution_started(
        &self,
        execution_id: Uuid,
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        let execution = self.assigned_executions.write().swap_remove(&execution_id);
        if let Some(mut execution) = execution {
            debug_assert!(execution.started_at.is_none());
            execution.started_at = Some(timestamp);

            self.started_executions
                .write()
                .insert(execution_id, execution);
        }

        Ok(())
    }

    async fn execution_succeeded(
        &self,
        execution_id: Uuid,
        timestamp: SystemTime,
        output_payload_json: String,
    ) -> eyre::Result<()> {
        // executions may succeed at any phase, so we need to check all phases
        let mut execution = if let Some(execution) =
            self.pending_executions.write().swap_remove(&execution_id)
        {
            execution
        } else if let Some(execution) = self.ready_executions.write().swap_remove(&execution_id) {
            execution
        } else if let Some(execution) = self.assigned_executions.write().swap_remove(&execution_id)
        {
            execution
        } else if let Some(execution) = self.started_executions.write().swap_remove(&execution_id) {
            execution
        } else {
            return Ok(());
        };

        debug_assert!(execution.succeeded_at.is_none());
        debug_assert!(execution.failed_at.is_none());

        execution.succeeded_at = Some(timestamp);
        execution.output_payload_json = Some(output_payload_json);

        let job = self.schedulable_jobs.write().swap_remove(&execution.job_id);
        if let Some(mut job) = job {
            debug_assert!(job.marked_unschedulable_at.is_none());
            job.marked_unschedulable_at = Some(timestamp);

            self.unschedulable_jobs
                .write()
                .insert(execution.job_id, job);
        }

        self.succeeded_executions
            .write()
            .insert(execution_id, execution);

        Ok(())
    }

    async fn executions_failed(
        &self,
        execution_ids: &[Uuid],
        timestamp: SystemTime,
        reason: String,
        mark_job_inactive: bool,
    ) -> eyre::Result<()> {
        // executions may fail at any phase, so we need to check all phases
        for execution_id in execution_ids {
            let mut execution = if let Some(execution) =
                self.pending_executions.write().swap_remove(execution_id)
            {
                execution
            } else if let Some(execution) = self.ready_executions.write().swap_remove(execution_id)
            {
                execution
            } else if let Some(execution) =
                self.assigned_executions.write().swap_remove(execution_id)
            {
                execution
            } else if let Some(execution) =
                self.started_executions.write().swap_remove(execution_id)
            {
                execution
            } else {
                return Ok(());
            };

            debug_assert!(execution.succeeded_at.is_none());
            debug_assert!(execution.failed_at.is_none());

            execution.failed_at = Some(timestamp);
            execution.failure_reason = Some(reason.clone());

            if mark_job_inactive {
                let job = self.schedulable_jobs.write().swap_remove(&execution.job_id);
                if let Some(mut job) = job {
                    debug_assert!(job.marked_unschedulable_at.is_none());
                    job.marked_unschedulable_at = Some(timestamp);

                    self.unschedulable_jobs
                        .write()
                        .insert(execution.job_id, job);
                }
            }

            self.failed_executions
                .write()
                .insert(*execution_id, execution);
        }

        Ok(())
    }

    async fn orphan_execution_ids(&self, executor_ids: &[Uuid]) -> eyre::Result<Vec<Uuid>> {
        Ok(self
            .assigned_executions
            .read()
            .iter()
            .filter_map(|(id, execution)| {
                if executor_ids.contains(&execution.executor_id.unwrap()) {
                    None
                } else {
                    Some(*id)
                }
            })
            .chain(
                self.started_executions
                    .read()
                    .iter()
                    .filter_map(|(id, execution)| {
                        if executor_ids.contains(&execution.executor_id.unwrap()) {
                            None
                        } else {
                            Some(*id)
                        }
                    }),
            )
            .collect())
    }

    async fn jobs_unschedulable(
        &self,
        job_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        for job_id in job_ids {
            let job = self.schedulable_jobs.write().swap_remove(job_id);
            if let Some(mut job) = job {
                debug_assert!(job.marked_unschedulable_at.is_none());
                job.marked_unschedulable_at = Some(timestamp);

                self.unschedulable_jobs.write().insert(*job_id, job);
            } else {
                debug_assert!(false, "active job with ID {job_id} not found");
            }
        }

        Ok(())
    }

    async fn pending_executions(&self, after: Option<Uuid>) -> eyre::Result<Vec<PendingExecution>> {
        Ok(self
            .pending_executions
            .read()
            .iter()
            .filter_map(|(id, execution)| {
                if let Some(after) = after {
                    if *id <= after {
                        return None;
                    }
                }

                Some(PendingExecution {
                    id: *id,
                    target_execution_time: execution.target_execution_time,
                })
            })
            .collect())
    }

    async fn ready_executions(&self, after: Option<Uuid>) -> eyre::Result<Vec<ReadyExecution>> {
        Ok({
            let mut executions = self
                .ready_executions
                .read()
                .iter()
                .filter_map(|(id, execution)| {
                    let jobs = self.schedulable_jobs.read();
                    let Some(job) = jobs.get(&execution.job_id) else {
                        debug_assert!(false, "active job with ID {} not found", execution.job_id);
                        return None;
                    };

                    Some(ReadyExecution {
                        id: *id,
                        target_execution_time: execution.target_execution_time,
                        job_id: execution.job_id,
                        input_payload_json: job.input_payload_json.clone(),
                        attempt_number: 0,
                        job_type_id: job.job_type_id.clone(),
                        timeout_policy: job.timeout_policy,
                    })
                })
                .collect::<Vec<_>>();

            executions.sort_by_key(|execution| execution.id);

            if let Some(after) = after {
                executions.retain(|execution| execution.id > after);
            }

            for execution in &mut executions {
                execution.attempt_number = u64::try_from(
                    self.executions_by_job_id(execution.job_id)
                        .position(|id| id == execution.id)
                        .unwrap(),
                )
                .unwrap()
                    + 1;
            }

            executions
        })
    }

    async fn pending_jobs(&self, after: Option<Uuid>) -> eyre::Result<Vec<PendingJob>> {
        let mut jobs: Vec<PendingJob> = {
            let pending_executions = self.pending_executions.read();
            let ready_executions = self.ready_executions.read();
            let assigned_executions = self.assigned_executions.read();
            let started_executions = self.started_executions.read();

            self.schedulable_jobs
                .read()
                .iter()
                .filter_map(|(job_id, job)| {
                    if pending_executions
                        .values()
                        .any(|execution| execution.job_id == *job_id)
                    {
                        return None;
                    }

                    if ready_executions
                        .values()
                        .any(|execution| execution.job_id == *job_id)
                    {
                        return None;
                    }

                    if assigned_executions
                        .values()
                        .any(|execution| execution.job_id == *job_id)
                    {
                        return None;
                    }

                    if started_executions
                        .values()
                        .any(|execution| execution.job_id == *job_id)
                    {
                        return None;
                    }

                    if let Some(after) = after {
                        if job.id <= after {
                            return None;
                        }
                    }

                    Some(PendingJob {
                        id: job.id,
                        target_execution_time: job.target_execution_time,
                        execution_count: 0,
                        retry_policy: job.retry_policy,
                        timeout_policy: job.timeout_policy,
                    })
                })
                .collect::<Vec<_>>()
        };

        for job in &mut jobs {
            job.execution_count = u64::try_from(self.executions_by_job_id(job.id).len()).unwrap();
        }

        Ok(jobs)
    }

    async fn query_jobs(
        &self,
        cursor: Option<String>,
        limit: usize,
        order: JobQueryOrder,
        filters: JobQueryFilters,
    ) -> eyre::Result<JobQueryResult> {
        let cursor: Option<job_query::Cursor> = match cursor {
            Some(cursor) => serde_json::from_str(&cursor).wrap_err("invalid cursor")?,
            None => None,
        };

        Ok(self.query_jobs_impl(cursor, limit, order, filters))
    }

    async fn query_job_ids(&self, filters: JobQueryFilters) -> eyre::Result<Vec<Uuid>> {
        Ok(self.query_job_ids_impl(filters))
    }

    async fn count_jobs(&self, filters: JobQueryFilters) -> eyre::Result<u64> {
        Ok(self.count_jobs_impl(filters))
    }

    async fn query_job_types(&self) -> eyre::Result<Vec<JobType>> {
        Ok(self.job_types.read().values().cloned().collect())
    }

    async fn delete_jobs(&self, filters: JobQueryFilters) -> eyre::Result<Vec<Uuid>> {
        let job_ids = self.query_job_ids_impl(filters);

        let mut executions_to_remove = Vec::new();

        for job_id in &job_ids {
            self.unschedulable_jobs
                .write()
                .swap_remove(job_id)
                .or_else(|| self.schedulable_jobs.write().swap_remove(job_id));

            executions_to_remove.extend(self.executions_by_job_id(*job_id));
        }

        self.pending_executions
            .write()
            .retain(|id, _| !executions_to_remove.contains(id));
        self.ready_executions
            .write()
            .retain(|id, _| !executions_to_remove.contains(id));
        self.assigned_executions
            .write()
            .retain(|id, _| !executions_to_remove.contains(id));
        self.started_executions
            .write()
            .retain(|id, _| !executions_to_remove.contains(id));
        self.succeeded_executions
            .write()
            .retain(|id, _| !executions_to_remove.contains(id));
        self.failed_executions
            .write()
            .retain(|id, _| !executions_to_remove.contains(id));

        Ok(job_ids)
    }

    async fn schedules_added(&self, schedules: Vec<NewSchedule>) -> eyre::Result<()> {
        for schedule in schedules {
            let mut active_schedules = self.schedulable_schedules.write();

            if active_schedules.contains_key(&schedule.id) {
                bail!("schedule with ID {} already exists", schedule.id);
            }

            active_schedules.insert(schedule.id, Schedule::from(schedule));
        }

        Ok(())
    }

    async fn schedules_cancelled(
        &self,
        schedule_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<Vec<CancelledSchedule>> {
        let mut cancelled_schedules = Vec::with_capacity(schedule_ids.len());

        for schedule_id in schedule_ids {
            let schedule = self.schedulable_schedules.write().swap_remove(schedule_id);

            if let Some(mut schedule) = schedule {
                debug_assert!(schedule.cancelled_at.is_none());
                schedule.cancelled_at = Some(timestamp);

                debug_assert!(schedule.marked_unschedulable_at.is_none());
                schedule.marked_unschedulable_at = Some(timestamp);

                let schedule_id = schedule.id;

                self.unschedulable_schedules
                    .write()
                    .insert(schedule_id, schedule);

                cancelled_schedules.push(CancelledSchedule { id: schedule_id });
            }
        }

        Ok(cancelled_schedules)
    }

    async fn pending_schedules(&self, after: Option<Uuid>) -> eyre::Result<Vec<PendingSchedule>> {
        Ok(self
            .schedulable_schedules
            .read()
            .iter()
            .filter_map(|(id, schedule)| {
                if self
                    .schedulable_jobs
                    .read()
                    .values()
                    .any(|job| job.schedule_id == Some(schedule.id))
                {
                    return None;
                }

                if let Some(after) = after {
                    if *id <= after {
                        return None;
                    }
                }

                Some(PendingSchedule {
                    id: *id,
                    job_timing_policy: schedule.job_timing_policy.clone(),
                    job_creation_policy: schedule.job_creation_policy.clone(),
                    last_target_execution_time: self.last_target_execution_time(schedule.id),
                    time_range: schedule.time_range,
                })
            })
            .collect::<Vec<_>>())
    }

    async fn query_schedules(
        &self,
        cursor: Option<String>,
        limit: usize,
        filters: ScheduleQueryFilters,
        order: ScheduleQueryOrder,
    ) -> eyre::Result<ScheduleQueryResult> {
        let cursor: Option<schedule_query::Cursor> = match cursor {
            Some(cursor) => serde_json::from_str(&cursor).wrap_err("invalid cursor")?,
            None => None,
        };

        Ok(self.query_schedules_impl(cursor, limit, order, filters))
    }

    async fn query_schedule_ids(&self, filters: ScheduleQueryFilters) -> eyre::Result<Vec<Uuid>> {
        Ok(self.query_schedule_ids_impl(filters))
    }

    async fn count_schedules(&self, filters: ScheduleQueryFilters) -> eyre::Result<u64> {
        Ok(self.count_schedules_impl(filters))
    }

    async fn schedules_unschedulable(
        &self,
        schedule_ids: &[Uuid],
        timestamp: SystemTime,
    ) -> eyre::Result<()> {
        for schedule_id in schedule_ids {
            let schedule = self.schedulable_schedules.write().swap_remove(schedule_id);

            if let Some(mut schedule) = schedule {
                debug_assert!(schedule.marked_unschedulable_at.is_none());
                schedule.marked_unschedulable_at = Some(timestamp);

                let schedule_id = schedule.id;

                self.unschedulable_schedules
                    .write()
                    .insert(schedule_id, schedule);
            }
        }

        Ok(())
    }

    async fn delete_schedules(&self, filters: ScheduleQueryFilters) -> eyre::Result<Vec<Uuid>> {
        let schedule_ids = self
            .query_schedule_ids_impl(filters)
            .into_iter()
            .collect::<IndexSet<_>>();

        // We don't care about orphaned jobs here.
        self.schedulable_schedules
            .write()
            .retain(|id, _| !schedule_ids.contains(id));
        self.unschedulable_schedules
            .write()
            .retain(|id, _| !schedule_ids.contains(id));

        Ok(schedule_ids.into_iter().collect())
    }
}

impl MemoryStorage {
    /// Returns all execution IDs for a job in creation order.
    fn executions_by_job_id(&self, job_id: Uuid) -> impl ExactSizeIterator<Item = Uuid> {
        let mut execution_ids = self
            .pending_executions
            .read()
            .values()
            .chain(self.ready_executions.read().values())
            .chain(self.assigned_executions.read().values())
            .chain(self.started_executions.read().values())
            .chain(self.succeeded_executions.read().values())
            .chain(self.failed_executions.read().values())
            .filter(move |execution| execution.job_id == job_id)
            .map(|execution| execution.id)
            .collect::<Vec<_>>();

        execution_ids.sort_unstable();

        execution_ids.into_iter()
    }

    /// Returns the last target execution time of a schedule.
    fn last_target_execution_time(&self, schedule_id: Uuid) -> Option<SystemTime> {
        self.schedulable_jobs
            .read()
            .values()
            .filter(|job| job.schedule_id == Some(schedule_id))
            .map(|job| job.target_execution_time)
            .max()
            .max(
                self.unschedulable_jobs
                    .read()
                    .values()
                    .filter(|job| job.schedule_id == Some(schedule_id))
                    .map(|job| job.target_execution_time)
                    .max(),
            )
    }
}

#[derive(Debug, Clone)]
struct Job {
    id: Uuid,
    schedule_id: Option<Uuid>,
    created_at: SystemTime,
    job_type_id: String,
    target_execution_time: SystemTime,
    retry_policy: JobRetryPolicy,
    timeout_policy: JobTimeoutPolicy,
    labels: IndexMap<String, String>,
    marked_unschedulable_at: Option<SystemTime>,
    cancelled_at: Option<SystemTime>,
    input_payload_json: String,
    metadata_json: Option<String>,
}

impl From<NewJob> for Job {
    fn from(job: NewJob) -> Self {
        Self {
            id: job.id,
            schedule_id: job.schedule_id,
            created_at: job.created_at,
            job_type_id: job.job_type_id,
            target_execution_time: job.target_execution_time,
            retry_policy: job.retry_policy,
            timeout_policy: job.timeout_policy,
            labels: job.labels,
            input_payload_json: job.input_payload_json,
            marked_unschedulable_at: None,
            cancelled_at: None,
            metadata_json: job.metadata_json,
        }
    }
}

#[derive(Debug, Clone)]
struct Execution {
    id: Uuid,
    job_id: Uuid,
    target_execution_time: SystemTime,
    executor_id: Option<Uuid>,
    created_at: SystemTime,
    ready_at: Option<SystemTime>,
    assigned_at: Option<SystemTime>,
    started_at: Option<SystemTime>,
    succeeded_at: Option<SystemTime>,
    failed_at: Option<SystemTime>,
    output_payload_json: Option<String>,
    failure_reason: Option<String>,
}

impl From<&Execution> for ExecutionDetails {
    fn from(value: &Execution) -> Self {
        Self {
            id: value.id,
            job_id: value.job_id,
            executor_id: value.executor_id,
            status: if value.succeeded_at.is_some() {
                JobExecutionStatus::Succeeded
            } else if value.failed_at.is_some() {
                JobExecutionStatus::Failed
            } else if value.started_at.is_some() {
                JobExecutionStatus::Running
            } else if value.assigned_at.is_some() {
                JobExecutionStatus::Assigned
            } else if value.ready_at.is_some() {
                JobExecutionStatus::Ready
            } else {
                JobExecutionStatus::Pending
            },
            created_at: value.created_at,
            ready_at: value.ready_at,
            assigned_at: value.assigned_at,
            started_at: value.started_at,
            succeeded_at: value.succeeded_at,
            failed_at: value.failed_at,
            output_payload_json: value.output_payload_json.clone(),
            failure_reason: value.failure_reason.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct Schedule {
    id: Uuid,
    created_at: SystemTime,
    job_type_id: Option<String>,
    labels: IndexMap<String, String>,
    marked_unschedulable_at: Option<SystemTime>,
    cancelled_at: Option<SystemTime>,
    job_timing_policy: ScheduleJobTimingPolicy,
    job_creation_policy: ScheduleJobCreationPolicy,
    time_range: Option<ScheduleTimeRange>,
    metadata_json: Option<String>,
}

impl From<NewSchedule> for Schedule {
    fn from(schedule: NewSchedule) -> Self {
        Self {
            id: schedule.id,
            created_at: schedule.created_at,
            job_type_id: match &schedule.job_creation_policy {
                ScheduleJobCreationPolicy::JobDefinition(schedule_new_job_definition) => {
                    Some(schedule_new_job_definition.job_type_id.clone())
                }
            },
            labels: schedule.labels,
            marked_unschedulable_at: None,
            cancelled_at: None,
            job_timing_policy: schedule.job_timing_policy,
            job_creation_policy: schedule.job_creation_policy,
            time_range: schedule.time_range,
            metadata_json: schedule.metadata_json,
        }
    }
}
