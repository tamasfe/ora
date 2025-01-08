use std::sync::Arc;

use tonic::async_trait;

use ora_storage::{
    CancelledJob, JobQueryFilters, JobQueryOrder, JobQueryResult, PendingSchedule,
    ScheduleQueryFilters, ScheduleQueryOrder, ScheduleQueryResult, Storage, StorageSnapshot,
};
use uuid::Uuid;

use crate::{events::EventBus, AuditEventKind};

/// A wrapper around a storage implementation that adds additional
/// functionality.
#[derive(Debug, Clone)]
pub(crate) struct StorageWrapper<S> {
    pub(super) inner: S,
    /// A lock to prevent concurrent imports as well as reading
    /// potentially inconsistent data during an import.
    import_lock: Arc<tokio::sync::RwLock<()>>,
    event_bus: EventBus,
}

impl<S> StorageWrapper<S> {
    /// Create a new observed storage instance.
    pub(crate) fn new(storage: S, event_bus: EventBus) -> Self {
        Self {
            inner: storage,
            event_bus,
            import_lock: Default::default(),
        }
    }
}

#[async_trait]
impl<S> Storage for StorageWrapper<S>
where
    S: Storage,
{
    async fn job_types_added(&self, job_types: Vec<ora_storage::JobType>) -> eyre::Result<()> {
        self.inner.job_types_added(job_types).await?;
        Ok(())
    }

    async fn jobs_added(&self, jobs: Vec<ora_storage::NewJob>) -> eyre::Result<()> {
        let audit_events = if self.event_bus.audit_events_enabled() {
            jobs.iter()
                .map(|job| AuditEventKind::JobAdded {
                    job_id: job.id,
                    job_type_id: job.job_type_id.clone(),
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        self.inner.jobs_added(jobs).await?;
        for event in audit_events {
            self.event_bus.emit_audit_event(|| event);
        }
        Ok(())
    }

    async fn jobs_cancelled(
        &self,
        job_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<Vec<CancelledJob>> {
        let _import_lock = self.import_lock.read().await;
        let cancelled_jobs = self.inner.jobs_cancelled(job_ids, timestamp).await?;

        if self.event_bus.audit_events_enabled() {
            for job_id in cancelled_jobs.iter().map(|j| j.id) {
                self.event_bus
                    .emit_audit_event(|| AuditEventKind::JobCancelled { job_id });
            }
        }

        Ok(cancelled_jobs)
    }

    async fn executions_added(
        &self,
        executions: Vec<ora_storage::NewExecution>,
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        let audit_events = if self.event_bus.audit_events_enabled() {
            executions
                .iter()
                .map(|execution| AuditEventKind::ExecutionAdded {
                    execution_id: execution.id,
                    job_id: execution.job_id,
                    target_execution_time: execution.target_execution_time,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        self.inner.executions_added(executions, timestamp).await?;

        for event in audit_events {
            self.event_bus.emit_audit_event(|| event);
        }

        Ok(())
    }

    async fn executions_ready(
        &self,
        execution_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        self.inner
            .executions_ready(execution_ids, timestamp)
            .await?;

        if self.event_bus.audit_events_enabled() {
            for execution_id in execution_ids {
                self.event_bus
                    .emit_audit_event(|| AuditEventKind::ExecutionReady {
                        execution_id: *execution_id,
                    });
            }
        }

        Ok(())
    }

    async fn execution_assigned(
        &self,
        execution_id: Uuid,
        executor_id: Uuid,
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        self.inner
            .execution_assigned(execution_id, executor_id, timestamp)
            .await?;

        if self.event_bus.audit_events_enabled() {
            self.event_bus
                .emit_audit_event(|| AuditEventKind::ExecutionAssigned {
                    execution_id,
                    executor_id,
                });
        }

        Ok(())
    }

    async fn execution_started(
        &self,
        execution_id: Uuid,
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        self.inner
            .execution_started(execution_id, timestamp)
            .await?;

        if self.event_bus.audit_events_enabled() {
            self.event_bus
                .emit_audit_event(|| AuditEventKind::ExecutionStarted { execution_id });
        }

        Ok(())
    }

    async fn execution_succeeded(
        &self,
        execution_id: Uuid,
        timestamp: std::time::SystemTime,
        output_payload_json: String,
    ) -> eyre::Result<()> {
        self.inner
            .execution_succeeded(execution_id, timestamp, output_payload_json)
            .await?;

        if self.event_bus.audit_events_enabled() {
            self.event_bus
                .emit_audit_event(|| AuditEventKind::ExecutionSucceeded { execution_id });
        }

        Ok(())
    }

    async fn executions_failed(
        &self,
        execution_ids: &[Uuid],
        timestamp: std::time::SystemTime,
        reason: String,
        mark_job_inactive: bool,
    ) -> eyre::Result<()> {
        self.inner
            .executions_failed(execution_ids, timestamp, reason, mark_job_inactive)
            .await?;

        if self.event_bus.audit_events_enabled() {
            for execution_id in execution_ids {
                self.event_bus
                    .emit_audit_event(|| AuditEventKind::ExecutionFailed {
                        execution_id: *execution_id,
                        terminal: mark_job_inactive,
                    });
            }
        }

        Ok(())
    }

    async fn orphan_execution_ids(&self, executor_ids: &[Uuid]) -> eyre::Result<Vec<Uuid>> {
        self.inner.orphan_execution_ids(executor_ids).await
    }

    async fn jobs_unschedulable(
        &self,
        job_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        self.inner.jobs_unschedulable(job_ids, timestamp).await?;
        Ok(())
    }

    async fn pending_executions(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::PendingExecution>> {
        let _import_lock = self.import_lock.read().await;
        self.inner.pending_executions(after).await
    }

    async fn ready_executions(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::ReadyExecution>> {
        let _import_lock = self.import_lock.read().await;
        self.inner.ready_executions(after).await
    }

    async fn pending_jobs(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::PendingJob>> {
        let _import_lock = self.import_lock.read().await;
        self.inner.pending_jobs(after).await
    }

    async fn query_jobs(
        &self,
        next_token: Option<String>,
        limit: usize,
        order: JobQueryOrder,
        filters: JobQueryFilters,
    ) -> eyre::Result<JobQueryResult> {
        let _import_lock = self.import_lock.read().await;
        self.inner
            .query_jobs(next_token, limit, order, filters)
            .await
    }

    async fn query_job_ids(&self, filters: JobQueryFilters) -> eyre::Result<Vec<Uuid>> {
        let _import_lock = self.import_lock.read().await;
        self.inner.query_job_ids(filters).await
    }

    async fn count_jobs(&self, filters: JobQueryFilters) -> eyre::Result<u64> {
        let _import_lock = self.import_lock.read().await;
        self.inner.count_jobs(filters).await
    }

    async fn query_job_types(&self) -> eyre::Result<Vec<ora_storage::JobType>> {
        self.inner.query_job_types().await
    }

    async fn delete_jobs(&self, filters: JobQueryFilters) -> eyre::Result<Vec<Uuid>> {
        let _import_lock = self.import_lock.read().await;
        let deleted_jobs = self.inner.delete_jobs(filters).await?;

        if self.event_bus.audit_events_enabled() {
            for job_id in &deleted_jobs {
                self.event_bus
                    .emit_audit_event(|| AuditEventKind::JobDeleted { job_id: *job_id });
            }
        }

        Ok(deleted_jobs)
    }

    async fn schedules_added(&self, schedules: Vec<ora_storage::NewSchedule>) -> eyre::Result<()> {
        let audit_events = if self.event_bus.audit_events_enabled() {
            schedules
                .iter()
                .map(|schedule| AuditEventKind::ScheduleAdded {
                    schedule_id: schedule.id,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        self.inner.schedules_added(schedules).await?;

        for event in audit_events {
            self.event_bus.emit_audit_event(|| event);
        }

        Ok(())
    }

    async fn schedules_cancelled(
        &self,
        schedule_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<Vec<ora_storage::CancelledSchedule>> {
        let _import_lock = self.import_lock.read().await;
        let cancelled_schedules = self
            .inner
            .schedules_cancelled(schedule_ids, timestamp)
            .await?;

        if self.event_bus.audit_events_enabled() {
            for schedule_id in cancelled_schedules.iter().map(|s| s.id) {
                self.event_bus
                    .emit_audit_event(|| AuditEventKind::ScheduleCancelled { schedule_id });
            }
        }

        Ok(cancelled_schedules)
    }

    async fn schedules_unschedulable(
        &self,
        schedule_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        self.inner
            .schedules_unschedulable(schedule_ids, timestamp)
            .await?;

        if self.event_bus.audit_events_enabled() {
            for schedule_id in schedule_ids {
                self.event_bus
                    .emit_audit_event(|| AuditEventKind::ScheduleUnschedulable {
                        schedule_id: *schedule_id,
                    });
            }
        }

        Ok(())
    }

    async fn pending_schedules(&self, after: Option<Uuid>) -> eyre::Result<Vec<PendingSchedule>> {
        let _import_lock = self.import_lock.read().await;
        self.inner.pending_schedules(after).await
    }

    async fn query_schedules(
        &self,
        cursor: Option<String>,
        limit: usize,
        filters: ScheduleQueryFilters,
        order: ScheduleQueryOrder,
    ) -> eyre::Result<ScheduleQueryResult> {
        let _import_lock = self.import_lock.read().await;
        self.inner
            .query_schedules(cursor, limit, filters, order)
            .await
    }

    async fn query_schedule_ids(&self, filters: ScheduleQueryFilters) -> eyre::Result<Vec<Uuid>> {
        let _import_lock = self.import_lock.read().await;
        self.inner.query_schedule_ids(filters).await
    }

    async fn count_schedules(&self, filters: ScheduleQueryFilters) -> eyre::Result<u64> {
        let _import_lock = self.import_lock.read().await;
        self.inner.count_schedules(filters).await
    }

    async fn delete_schedules(&self, filters: ScheduleQueryFilters) -> eyre::Result<Vec<Uuid>> {
        let _import_lock = self.import_lock.read().await;
        let deleted_schedules = self.inner.delete_schedules(filters).await?;

        if self.event_bus.audit_events_enabled() {
            for schedule_id in &deleted_schedules {
                self.event_bus
                    .emit_audit_event(|| AuditEventKind::ScheduleDeleted {
                        schedule_id: *schedule_id,
                    });
            }
        }

        Ok(deleted_schedules)
    }
}

#[async_trait]
impl<S> StorageSnapshot for StorageWrapper<S>
where
    S: StorageSnapshot + Send + Sync,
{
    fn export_snapshot(
        &self,
    ) -> futures::stream::BoxStream<'static, eyre::Result<ora_proto::snapshot::v1::SnapshotData>>
    {
        self.inner.export_snapshot()
    }

    async fn import_snapshot(
        &self,
        snapshot: futures::stream::BoxStream<
            'static,
            eyre::Result<ora_proto::snapshot::v1::SnapshotData>,
        >,
    ) -> eyre::Result<()> {
        let _import_lock = self.import_lock.write().await;
        self.inner.import_snapshot(snapshot).await
    }
}
