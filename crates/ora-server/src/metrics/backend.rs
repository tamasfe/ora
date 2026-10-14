//! A backend wrapper recording metrics of backend operations.

use std::{collections::HashSet, time::SystemTime};

use futures::{Stream, TryStreamExt};
use ora_backend::{
    Backend,
    common::NextPageToken,
    executions::{
        ExecutionId, FailedExecution, InProgressExecution, ReadyExecution, RetriedExecution,
        StartedExecution, SucceededExecution,
    },
    jobs::{AddedJobs, CancelledJob, JobDetails, JobFilters, JobOrderBy, JobType, NewJob},
    schedules::{
        AddedSchedules, PendingSchedule, ScheduleDefinition, ScheduleDetails, ScheduleFilters,
        ScheduleOrderBy, StoppedSchedule,
    },
};

use super::{JobOutcome, JobSource, backend_error, job_added, job_finished};

/// A backend wrapper that records metrics for every backend operation,
/// regardless of where it is called from.
#[derive(Debug)]
pub(crate) struct InstrumentedBackend<B> {
    backend: B,
}

impl<B> InstrumentedBackend<B> {
    pub(crate) fn new(backend: B) -> Self {
        Self { backend }
    }
}

fn record<T, E>(operation: &'static str, result: Result<T, E>) -> Result<T, E> {
    if result.is_err() {
        backend_error(operation);
    }
    result
}

impl<B: Backend> Backend for InstrumentedBackend<B> {
    type Error = B::Error;

    async fn add_job_types(&self, job_types: &[JobType]) -> Result<(), Self::Error> {
        record("add_job_types", self.backend.add_job_types(job_types).await)
    }

    async fn list_job_types(&self) -> Result<Vec<JobType>, Self::Error> {
        record("list_job_types", self.backend.list_job_types().await)
    }

    async fn add_jobs(
        &self,
        jobs: &[NewJob],
        if_not_exists: Option<JobFilters>,
    ) -> Result<AddedJobs, Self::Error> {
        let added = record("add_jobs", self.backend.add_jobs(jobs, if_not_exists).await)?;

        if let AddedJobs::Added(_) = &added {
            for job in jobs {
                let source = match job.schedule_id {
                    Some(_) => JobSource::Schedule,
                    None => JobSource::Api,
                };
                job_added(&job.job.job_type_id, source);
            }
        }

        Ok(added)
    }

    async fn list_jobs(
        &self,
        filters: JobFilters,
        order_by: Option<JobOrderBy>,
        page_size: u32,
        page_token: Option<NextPageToken>,
    ) -> Result<(Vec<JobDetails>, Option<NextPageToken>), Self::Error> {
        record(
            "list_jobs",
            self.backend
                .list_jobs(filters, order_by, page_size, page_token)
                .await,
        )
    }

    async fn count_jobs(&self, filters: JobFilters) -> Result<u64, Self::Error> {
        record("count_jobs", self.backend.count_jobs(filters).await)
    }

    async fn cancel_jobs(&self, filters: JobFilters) -> Result<Vec<CancelledJob>, Self::Error> {
        let cancelled = record("cancel_jobs", self.backend.cancel_jobs(filters).await)?;

        // The backend returns a row per execution, count each job only once.
        let mut counted_job_ids = HashSet::new();
        for job in &cancelled {
            if counted_job_ids.insert(job.job_id) {
                job_finished(&job.job_type_id, JobOutcome::Cancelled);
            }
        }

        Ok(cancelled)
    }

    async fn add_schedules(
        &self,
        schedules: &[ScheduleDefinition],
        if_not_exists: Option<ScheduleFilters>,
    ) -> Result<AddedSchedules, Self::Error> {
        record(
            "add_schedules",
            self.backend.add_schedules(schedules, if_not_exists).await,
        )
    }

    async fn list_schedules(
        &self,
        filters: ScheduleFilters,
        order_by: Option<ScheduleOrderBy>,
        page_size: u32,
        page_token: Option<NextPageToken>,
    ) -> Result<(Vec<ScheduleDetails>, Option<NextPageToken>), Self::Error> {
        record(
            "list_schedules",
            self.backend
                .list_schedules(filters, order_by, page_size, page_token)
                .await,
        )
    }

    async fn count_schedules(&self, filters: ScheduleFilters) -> Result<u64, Self::Error> {
        record(
            "count_schedules",
            self.backend.count_schedules(filters).await,
        )
    }

    async fn stop_schedules(
        &self,
        filters: ScheduleFilters,
    ) -> Result<Vec<StoppedSchedule>, Self::Error> {
        record("stop_schedules", self.backend.stop_schedules(filters).await)
    }

    fn ready_executions(
        &self,
        ignore: &[ExecutionId],
    ) -> impl Stream<Item = Result<Vec<ReadyExecution>, Self::Error>> + Send {
        self.backend
            .ready_executions(ignore)
            .inspect_err(|_| backend_error("ready_executions"))
    }

    async fn wait_for_ready_executions(&self, ignore: &[ExecutionId]) -> Result<(), Self::Error> {
        record(
            "wait_for_ready_executions",
            self.backend.wait_for_ready_executions(ignore).await,
        )
    }

    fn in_progress_executions(
        &self,
    ) -> impl Stream<Item = Result<Vec<InProgressExecution>, Self::Error>> + Send {
        self.backend
            .in_progress_executions()
            .inspect_err(|_| backend_error("in_progress_executions"))
    }

    async fn executions_started(
        &self,
        executions: &[StartedExecution],
    ) -> Result<Vec<ExecutionId>, Self::Error> {
        record(
            "executions_started",
            self.backend.executions_started(executions).await,
        )
    }

    async fn executions_succeeded(
        &self,
        executions: &[SucceededExecution],
    ) -> Result<(), Self::Error> {
        record(
            "executions_succeeded",
            self.backend.executions_succeeded(executions).await,
        )
    }

    async fn executions_failed(&self, executions: &[FailedExecution]) -> Result<(), Self::Error> {
        record(
            "executions_failed",
            self.backend.executions_failed(executions).await,
        )
    }

    async fn executions_retried(&self, executions: &[RetriedExecution]) -> Result<(), Self::Error> {
        record(
            "executions_retried",
            self.backend.executions_retried(executions).await,
        )
    }

    fn pending_schedules(
        &self,
    ) -> impl Stream<Item = Result<Vec<PendingSchedule>, Self::Error>> + Send {
        self.backend
            .pending_schedules()
            .inspect_err(|_| backend_error("pending_schedules"))
    }

    async fn wait_for_pending_schedules(&self) -> Result<(), Self::Error> {
        record(
            "wait_for_pending_schedules",
            self.backend.wait_for_pending_schedules().await,
        )
    }

    async fn delete_history(&self, before: SystemTime) -> Result<(), Self::Error> {
        record("delete_history", self.backend.delete_history(before).await)
    }
}
