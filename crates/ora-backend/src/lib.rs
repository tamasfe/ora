//! Common storage and timer backend interfaces for the Ora server.

use std::future::Future;

use futures::Stream;

use crate::{
    common::NextPageToken,
    executions::{
        FailedExecution, InProgressExecution, ReadyExecution, StartedExecution, SucceededExecution,
    },
    jobs::{CancelledJob, JobDetails, JobFilters, JobId, JobOrderBy, JobType, NewJob},
    schedules::{
        PendingSchedule, ScheduleDefinition, ScheduleDetails, ScheduleFilters, ScheduleId,
        ScheduleOrderBy, StoppedSchedule,
    },
};

pub mod common;
pub mod executions;
pub mod executors;
pub mod jobs;
pub mod schedules;

#[cfg(feature = "test")]
pub mod test;

/// Backend implementation for the Ora server.
///
/// A backend is responsible for all storage operations as well
/// as timing of executions.
pub trait Backend: Send + Sync + 'static {
    /// The error type for backend operations.
    type Error: core::error::Error + Send + Sync + 'static;

    /// Add or update job types.
    fn add_job_types(
        &self,
        job_types: &[JobType],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// List all job types.
    fn list_job_types(&self) -> impl Future<Output = Result<Vec<JobType>, Self::Error>> + Send;

    /// Add new jobs and return their IDs in the same order.
    ///
    /// For each new job an execution must also be created.
    ///
    /// If `if_not_exists` is provided and any jobs exist
    /// matching the given filters, no new jobs must be added
    /// and an empty list must be returned.
    fn add_jobs(
        &self,
        jobs: &[NewJob],
        if_not_exists: Option<JobFilters>,
    ) -> impl Future<Output = Result<Vec<JobId>, Self::Error>> + Send;

    /// List jobs matching the given filters.
    fn list_jobs(
        &self,
        filters: JobFilters,
        order_by: Option<JobOrderBy>,
        page_size: u32,
        page_token: Option<NextPageToken>,
    ) -> impl Future<Output = Result<(Vec<JobDetails>, Option<NextPageToken>), Self::Error>> + Send;

    /// Count the jobs matching the given filters.
    fn count_jobs(
        &self,
        filters: JobFilters,
    ) -> impl Future<Output = Result<u64, Self::Error>> + Send;

    /// Cancel the jobs matching the given filters, returning
    /// the cancelled jobs.
    fn cancel_jobs(
        &self,
        filters: JobFilters,
    ) -> impl Future<Output = Result<Vec<CancelledJob>, Self::Error>> + Send;

    /// Add new schedules and return their IDs in the same order.
    ///
    /// If `if_not_exists` is provided and any schedules exist
    /// matching the given filters, no new schedules must be added
    /// and an empty list must be returned.
    fn add_schedules(
        &self,
        schedules: &[ScheduleDefinition],
        if_not_exists: Option<ScheduleFilters>,
    ) -> impl Future<Output = Result<Vec<ScheduleId>, Self::Error>> + Send;

    /// List schedules matching the given filters.
    fn list_schedules(
        &self,
        filters: ScheduleFilters,
        order_by: Option<ScheduleOrderBy>,
        page_size: u32,
        page_token: Option<NextPageToken>,
    ) -> impl Future<Output = Result<(Vec<ScheduleDetails>, Option<NextPageToken>), Self::Error>> + Send;

    /// Count the schedules matching the given filters.
    fn count_schedules(
        &self,
        filters: ScheduleFilters,
    ) -> impl Future<Output = Result<u64, Self::Error>> + Send;

    /// Stop the schedules matching the given filters, returning
    /// the IDs of the stopped schedules.
    fn stop_schedules(
        &self,
        filters: ScheduleFilters,
    ) -> impl Future<Output = Result<Vec<StoppedSchedule>, Self::Error>> + Send;

    /// Return ready executions.
    ///
    /// The executions must be ordered by their ID.
    ///
    /// The batch size of ready executions returned by the stream
    /// is backend-dependent.
    fn ready_executions(
        &self,
    ) -> impl Stream<Item = Result<Vec<ReadyExecution>, Self::Error>> + Send;

    /// Wait for executions to be ready.
    fn wait_for_ready_executions(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Return a stream of in-progress executions.
    ///
    /// These are executions that have been started by executors
    /// but have not yet completed, failed, or been cancelled.
    fn in_progress_executions(
        &self,
    ) -> impl Stream<Item = Result<Vec<InProgressExecution>, Self::Error>> + Send;

    /// The given executions have started.
    ///
    /// The backend must update the execution records accordingly.
    fn executions_started(
        &self,
        executions: &[StartedExecution],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// The given executions have successfully completed.
    fn executions_succeeded(
        &self,
        executions: &[SucceededExecution],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// The given executions have failed.
    fn executions_failed(
        &self,
        executions: &[FailedExecution],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// The given executions have failed but need to be retried.
    ///
    /// For each job, a new execution must be created.
    fn executions_retried(
        &self,
        executions: &[FailedExecution],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Return a stream of active schedules that do not have
    /// an active job.
    fn pending_schedules(
        &self,
    ) -> impl Stream<Item = Result<Vec<PendingSchedule>, Self::Error>> + Send;

    /// Wait for pending schedules to be available.
    fn wait_for_pending_schedules(&self) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Delete historical data for inactive jobs and schedules.
    ///
    /// The following should be deleted:
    /// - Jobs with executions that finished before the given time.
    /// - Schedules that were stopped before the given time.
    /// - Delete job types that are not referenced by any jobs.
    fn delete_history(
        &self,
        before: std::time::SystemTime,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
