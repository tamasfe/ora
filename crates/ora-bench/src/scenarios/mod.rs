//! Benchmark scenarios and shared helpers.

use std::time::{Duration, Instant, SystemTime};

use eyre::{Context, OptionExt, bail};
use ora::proto::{
    admin::v1::{
        AddJobsRequest, CancelJobsRequest, JobFilters, admin_service_client::AdminServiceClient,
    },
    common::v1::{Label, LabelFilter},
    jobs::v1::{Job, RetryPolicy},
};
use tokio::sync::mpsc;
use tonic::transport::Channel;
use uuid::Uuid;

use crate::executor::{Event, JOB_TYPE_ID, Mode, Payload};

pub mod cancel;
pub mod delayed;
pub mod latency;
pub mod retry;
pub mod rpc;
pub mod throughput;

/// The label used to tag jobs of a single scenario run.
const RUN_LABEL: &str = "ora_bench_run";

/// How long to wait for a single executor event
/// before giving up.
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);

/// Shared state for running scenarios.
pub struct Ctx {
    pub admin: AdminServiceClient<Channel>,
    pub events: mpsc::UnboundedReceiver<Event>,
    pub warmup: usize,
}

impl Ctx {
    /// Add jobs and return their IDs in order.
    pub async fn add_jobs(&mut self, jobs: Vec<Job>) -> eyre::Result<Vec<String>> {
        add_jobs(&mut self.admin, jobs).await
    }

    /// Wait for the next executor event.
    pub async fn next_event(&mut self, timeout: Duration) -> eyre::Result<Event> {
        tokio::time::timeout(timeout, self.events.recv())
            .await
            .wrap_err("timed out waiting for executor events")?
            .ok_or_eyre("the benchmark executor disconnected")
    }

    /// Wait for the first event matching the given function,
    /// discarding all other events.
    pub async fn wait_for<T>(&mut self, mut f: impl FnMut(Event) -> Option<T>) -> eyre::Result<T> {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Some(v) = f(self.next_event(remaining).await?) {
                return Ok(v);
            }
        }
    }

    /// Discard any queued events, e.g. left over from previous scenarios.
    pub fn drain_events(&mut self) {
        while self.events.try_recv().is_ok() {}
    }

    /// Cancel all active jobs matching the filters.
    pub async fn cancel(&mut self, filters: JobFilters) -> eyre::Result<usize> {
        let res = self
            .admin
            .cancel_jobs(CancelJobsRequest {
                filters: Some(filters),
            })
            .await?
            .into_inner();
        Ok(res.cancelled_job_ids.len())
    }
}

/// Add jobs and return their IDs in order.
pub async fn add_jobs(
    admin: &mut AdminServiceClient<Channel>,
    jobs: Vec<Job>,
) -> eyre::Result<Vec<String>> {
    let count = jobs.len();
    let res = admin
        .add_jobs(AddJobsRequest {
            jobs,
            if_not_exists: None,
        })
        .await?
        .into_inner();

    if res.job_ids.len() != count {
        bail!(
            "expected {count} jobs to be added, got {}",
            res.job_ids.len()
        );
    }

    Ok(res.job_ids)
}

/// A single scenario run, all jobs are labelled
/// with its ID.
pub struct Run {
    id: String,
}

impl Run {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
        }
    }

    /// A benchmark job for this run.
    pub fn job(&self, target_execution_time: SystemTime, mode: Mode) -> Job {
        Job {
            job_type_id: JOB_TYPE_ID.into(),
            target_execution_time: Some(target_execution_time.into()),
            input_payload_json: serde_json::to_string(&Payload { mode }).unwrap(),
            labels: vec![Label {
                key: RUN_LABEL.into(),
                value: self.id.clone(),
            }],
            timeout_policy: None,
            retry_policy: (mode == Mode::FailFirst).then_some(RetryPolicy {
                retries: 1,
                ..Default::default()
            }),
            priority: 0,
        }
    }

    /// Filters matching all jobs of this run.
    pub fn filters(&self) -> JobFilters {
        JobFilters {
            labels: vec![LabelFilter {
                key: RUN_LABEL.into(),
                value: Some(self.id.clone()),
            }],
            ..Default::default()
        }
    }
}

/// Filters matching all benchmark jobs, including ones from previous runs.
pub fn all_bench_jobs() -> JobFilters {
    JobFilters {
        job_type_ids: vec![JOB_TYPE_ID.into()],
        ..Default::default()
    }
}
