//! Metrics recorded by the server.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Once},
    time::{Duration, SystemTime},
};

use ora_backend::{
    Backend,
    executions::ExecutionStatus,
    jobs::{JobFilters, JobTypeId},
};
use wgroup::WaitGuard;

use crate::executor_pool::ExecutorPool;

pub(crate) const JOBS_ADDED_TOTAL: &str = "ora_jobs_added_total";
pub(crate) const JOBS_FINISHED_TOTAL: &str = "ora_jobs_finished_total";
pub(crate) const JOBS: &str = "ora_jobs";
pub(crate) const EXECUTIONS_STARTED_TOTAL: &str = "ora_executions_started_total";
pub(crate) const EXECUTIONS_FAILED_TOTAL: &str = "ora_executions_failed_total";
pub(crate) const EXECUTIONS_RETRIED_TOTAL: &str = "ora_executions_retried_total";
pub(crate) const EXECUTION_START_DELAY_SECONDS: &str = "ora_execution_start_delay_seconds";
pub(crate) const EXECUTION_DURATION_SECONDS: &str = "ora_execution_duration_seconds";
pub(crate) const EXECUTIONS_UNASSIGNED: &str = "ora_executions_unassigned";
pub(crate) const EXECUTIONS_ACTIVE: &str = "ora_executions_active";
pub(crate) const EXECUTOR_CAPACITY: &str = "ora_executor_capacity";
pub(crate) const EXECUTORS_CONNECTED: &str = "ora_executors_connected";
pub(crate) const BACKEND_ERRORS_TOTAL: &str = "ora_backend_errors_total";

/// How often in-memory executor pool state is sampled.
const EXECUTOR_POOL_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

/// The final outcome of a job.
#[derive(Debug, Clone, Copy)]
pub(crate) enum JobOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl JobOutcome {
    fn as_str(self) -> &'static str {
        match self {
            JobOutcome::Succeeded => "succeeded",
            JobOutcome::Failed => "failed",
            JobOutcome::Cancelled => "cancelled",
        }
    }
}

/// The reason an execution failed.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FailureKind {
    /// The executor reported a failure.
    Error,
    /// The executor returned an invalid output.
    InvalidOutput,
    /// The execution timed out.
    Timeout,
    /// The executor disconnected while running the execution.
    ExecutorDisconnected,
}

impl FailureKind {
    fn as_str(self) -> &'static str {
        match self {
            FailureKind::Error => "error",
            FailureKind::InvalidOutput => "invalid_output",
            FailureKind::Timeout => "timeout",
            FailureKind::ExecutorDisconnected => "executor_disconnected",
        }
    }
}

/// Where new jobs came from.
#[derive(Debug, Clone, Copy)]
pub(crate) enum JobSource {
    Api,
    Schedule,
}

impl JobSource {
    fn as_str(self) -> &'static str {
        match self {
            JobSource::Api => "api",
            JobSource::Schedule => "schedule",
        }
    }
}

/// Register metric descriptions, only once per process.
pub(crate) fn describe() {
    static DESCRIBE: Once = Once::new();

    DESCRIBE.call_once(|| {
        use metrics::{Unit, describe_counter, describe_gauge, describe_histogram};

        describe_counter!(
            JOBS_ADDED_TOTAL,
            Unit::Count,
            "The count of jobs added, by job type and source (api or schedule)."
        );
        describe_counter!(
            JOBS_FINISHED_TOTAL,
            Unit::Count,
            "The count of jobs that reached a final state, by job type and outcome (succeeded, failed or cancelled)."
        );
        describe_gauge!(
            JOBS,
            Unit::Count,
            "The count of unfinished jobs, by job type and status (pending or in_progress)."
        );
        describe_counter!(
            EXECUTIONS_STARTED_TOTAL,
            Unit::Count,
            "The count of executions assigned to executors, by job type."
        );
        describe_counter!(
            EXECUTIONS_FAILED_TOTAL,
            Unit::Count,
            "The count of failed executions (including ones that are retried), by job type and reason."
        );
        describe_counter!(
            EXECUTIONS_RETRIED_TOTAL,
            Unit::Count,
            "The count of failed executions that were scheduled for a retry, by job type."
        );
        describe_histogram!(
            EXECUTION_START_DELAY_SECONDS,
            Unit::Seconds,
            "The delay between the target execution time and the actual start of executions, by job type."
        );
        describe_histogram!(
            EXECUTION_DURATION_SECONDS,
            Unit::Seconds,
            "The duration of finished executions, by job type and outcome (succeeded or failed)."
        );
        describe_gauge!(
            EXECUTIONS_UNASSIGNED,
            Unit::Count,
            "The count of ready executions that are not yet assigned to any executor, by job type."
        );
        describe_gauge!(
            EXECUTIONS_ACTIVE,
            Unit::Count,
            "The count of executions currently assigned to executors, by job type."
        );
        describe_gauge!(
            EXECUTOR_CAPACITY,
            Unit::Count,
            "The maximum count of concurrent executions of all connected executors, by job type."
        );
        describe_gauge!(
            EXECUTORS_CONNECTED,
            Unit::Count,
            "The count of connected executors."
        );
        describe_counter!(
            BACKEND_ERRORS_TOTAL,
            Unit::Count,
            "The count of backend operation errors, by operation."
        );
    });
}

pub(crate) fn job_added(job_type_id: &JobTypeId, source: JobSource) {
    metrics::counter!(
        JOBS_ADDED_TOTAL,
        "job_type" => job_type_id.to_string(),
        "source" => source.as_str(),
    )
    .increment(1);
}

pub(crate) fn job_finished(job_type_id: &JobTypeId, outcome: JobOutcome) {
    metrics::counter!(
        JOBS_FINISHED_TOTAL,
        "job_type" => job_type_id.to_string(),
        "outcome" => outcome.as_str(),
    )
    .increment(1);
}

pub(crate) fn execution_started(
    job_type_id: &JobTypeId,
    target_execution_time: SystemTime,
    started_at: SystemTime,
) {
    let job_type = job_type_id.to_string();

    metrics::counter!(EXECUTIONS_STARTED_TOTAL, "job_type" => job_type.clone()).increment(1);
    metrics::histogram!(EXECUTION_START_DELAY_SECONDS, "job_type" => job_type)
        .record(seconds_between(target_execution_time, started_at));
}

pub(crate) fn execution_succeeded(
    job_type_id: &JobTypeId,
    started_at: SystemTime,
    succeeded_at: SystemTime,
) {
    metrics::histogram!(
        EXECUTION_DURATION_SECONDS,
        "job_type" => job_type_id.to_string(),
        "outcome" => "succeeded",
    )
    .record(seconds_between(started_at, succeeded_at));
}

pub(crate) fn execution_failed(
    job_type_id: &JobTypeId,
    kind: FailureKind,
    started_at: SystemTime,
    failed_at: SystemTime,
) {
    let job_type = job_type_id.to_string();

    metrics::counter!(
        EXECUTIONS_FAILED_TOTAL,
        "job_type" => job_type.clone(),
        "reason" => kind.as_str(),
    )
    .increment(1);
    metrics::histogram!(
        EXECUTION_DURATION_SECONDS,
        "job_type" => job_type,
        "outcome" => "failed",
    )
    .record(seconds_between(started_at, failed_at));
}

pub(crate) fn execution_retried(job_type_id: &JobTypeId) {
    metrics::counter!(EXECUTIONS_RETRIED_TOTAL, "job_type" => job_type_id.to_string()).increment(1);
}

pub(crate) fn backend_error(operation: &'static str) {
    metrics::counter!(BACKEND_ERRORS_TOTAL, "operation" => operation).increment(1);
}

fn seconds_between(from: SystemTime, to: SystemTime) -> f64 {
    to.duration_since(from).unwrap_or_default().as_secs_f64()
}

/// A set of gauges labelled by job type.
///
/// Job types that are no longer reported are reset to zero
/// instead of keeping their last value.
#[derive(Debug)]
pub(crate) struct JobTypeGauges {
    name: &'static str,
    extra_labels: Vec<(&'static str, &'static str)>,
    seen: HashSet<JobTypeId>,
}

impl JobTypeGauges {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            extra_labels: Vec::new(),
            seen: HashSet::new(),
        }
    }

    pub(crate) fn with_label(mut self, key: &'static str, value: &'static str) -> Self {
        self.extra_labels.push((key, value));
        self
    }

    pub(crate) fn set(&mut self, values: HashMap<JobTypeId, f64>) {
        for job_type_id in self.seen.difference(&values.keys().cloned().collect()) {
            self.gauge(job_type_id).set(0.0);
        }

        for (job_type_id, value) in &values {
            self.gauge(job_type_id).set(*value);
        }

        self.seen = values.into_keys().collect();
    }

    fn gauge(&self, job_type_id: &JobTypeId) -> metrics::Gauge {
        let mut labels = vec![metrics::Label::new("job_type", job_type_id.to_string())];
        labels.extend(
            self.extra_labels
                .iter()
                .map(|(k, v)| metrics::Label::new(*k, *v)),
        );
        metrics::gauge!(self.name, labels)
    }
}

/// Periodically sample the executor pool and update related gauges.
#[tracing::instrument(skip_all)]
pub(crate) async fn executor_pool_metrics_loop(executor_pool: ExecutorPool, wg: WaitGuard) {
    let mut active = JobTypeGauges::new(EXECUTIONS_ACTIVE);
    let mut capacity = JobTypeGauges::new(EXECUTOR_CAPACITY);

    loop {
        let stats = executor_pool.stats();

        #[allow(clippy::cast_precision_loss)]
        {
            metrics::gauge!(EXECUTORS_CONNECTED).set(stats.executor_count as f64);
        }
        active.set(stats.active_executions);
        capacity.set(stats.capacity);

        tokio::select! {
            _ = tokio::time::sleep(EXECUTOR_POOL_SAMPLE_INTERVAL) => {}
            _ = wg.waiting() => {
                tracing::debug!("shutting down");
                break;
            }
        }
    }
}

/// Periodically count unfinished jobs and unassigned executions in the backend.
#[tracing::instrument(skip_all)]
pub(crate) async fn job_counts_metrics_loop(
    backend: Arc<impl Backend>,
    executor_pool: ExecutorPool,
    interval: Duration,
    wg: WaitGuard,
) {
    let mut pending = JobTypeGauges::new(JOBS).with_label("status", "pending");
    let mut in_progress = JobTypeGauges::new(JOBS).with_label("status", "in_progress");
    let mut unassigned = JobTypeGauges::new(EXECUTIONS_UNASSIGNED);

    loop {
        match count_unfinished_jobs(&*backend).await {
            Ok((pending_counts, in_progress_counts)) => {
                pending.set(pending_counts);
                in_progress.set(in_progress_counts);
            }
            Err(error) => {
                backend_error("count_jobs");
                tracing::error!(%error, "failed to count jobs for metrics");
            }
        }

        match backend.count_ready_executions().await {
            Ok(ready_counts) => {
                unassigned.set(unassigned_counts(ready_counts, &executor_pool));
            }
            Err(error) => {
                backend_error("count_ready_executions");
                tracing::error!(%error, "failed to count ready executions for metrics");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = wg.waiting() => {
                tracing::debug!("shutting down");
                break;
            }
        }
    }
}

type JobCounts = HashMap<JobTypeId, f64>;

/// Ready executions in the backend minus the ones
/// already offered to (or accepted by) executors,
/// as those are still pending in the backend.
fn unassigned_counts(
    ready_counts: Vec<(JobTypeId, u64)>,
    executor_pool: &ExecutorPool,
) -> JobCounts {
    let in_flight = executor_pool.stats().in_flight_executions;

    ready_counts
        .into_iter()
        .map(|(job_type_id, count)| {
            #[allow(clippy::cast_precision_loss)]
            let count = count as f64 - in_flight.get(&job_type_id).copied().unwrap_or_default();
            (job_type_id, count.max(0.0))
        })
        .collect()
}

async fn count_unfinished_jobs<B: Backend>(
    backend: &B,
) -> Result<(JobCounts, JobCounts), B::Error> {
    let mut pending = HashMap::new();
    let mut in_progress = HashMap::new();

    for job_type in backend.list_job_types().await? {
        for (status, counts) in [
            (ExecutionStatus::Pending, &mut pending),
            (ExecutionStatus::InProgress, &mut in_progress),
        ] {
            let count = backend
                .count_jobs(JobFilters {
                    job_type_ids: Some(vec![job_type.id.clone()]),
                    execution_statuses: Some(vec![status]),
                    ..Default::default()
                })
                .await?;

            #[allow(clippy::cast_precision_loss)]
            counts.insert(job_type.id.clone(), count as f64);
        }
    }

    Ok((pending, in_progress))
}
