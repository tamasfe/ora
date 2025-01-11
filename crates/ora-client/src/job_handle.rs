//! Wrappers for interacting with jobs.

use std::{future::IntoFuture, sync::Arc};

use eyre::{bail, Context, OptionExt};
use futures::{future::BoxFuture, FutureExt};
use ora_proto::server::v1::admin_service_client::AdminServiceClient;
use parking_lot::Mutex;
use tonic::transport::Channel;
use uuid::Uuid;

use crate::{
    job_definition::{JobDetails, JobStatus},
    job_query::JobFilter,
    JobType,
};
#[allow(clippy::wildcard_imports)]
use tonic::codegen::*;

/// A handle to a single job.
///
/// The handle also caches the details of the job, so that
/// they can be accessed without making a network request.
#[derive(Debug)]
pub struct JobHandle<J = (), C = Channel> {
    id: Uuid,
    client: AdminServiceClient<C>,
    details: Arc<Mutex<Option<Arc<JobDetails>>>>,
    _job_type: std::marker::PhantomData<J>,
}

impl<J> Clone for JobHandle<J> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            client: self.client.clone(),
            details: self.details.clone(),
            _job_type: self._job_type,
        }
    }
}

impl<J, C> JobHandle<J, C>
where
    C: tonic::client::GrpcService<tonic::body::BoxBody> + Clone,
    C::Error: Into<StdError>,
    C::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
    <C::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
{
    /// Create a new job handle.
    pub(crate) fn new(id: Uuid, client: AdminServiceClient<C>) -> Self {
        Self {
            id,
            client,
            details: Arc::new(Mutex::new(None)),
            _job_type: std::marker::PhantomData,
        }
    }

    pub(crate) fn set_details(&self, details: Arc<JobDetails>) {
        *self.details.lock() = Some(details);
    }

    /// Get the ID of the job.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get the details of the job.
    pub async fn details(&self) -> eyre::Result<Arc<JobDetails>> {
        {
            let details = self.details.lock();

            if let Some(details) = &*details {
                if !details.active {
                    return Ok(details.clone());
                }
            }
        }

        let job = self
            .client
            .clone()
            .list_jobs(tonic::Request::new(
                ora_proto::server::v1::ListJobsRequest {
                    cursor: None,
                    limit: 1,
                    order: None,
                    filter: Some(JobFilter::new().with_job_id(self.id).into()),
                },
            ))
            .await?
            .into_inner()
            .jobs
            .into_iter()
            .next()
            .ok_or_else(|| eyre::eyre!("Job details not found"))?;

        let details = Arc::new(JobDetails::try_from(job)?);
        self.set_details(details.clone());

        Ok(details)
    }

    /// Get the status of the job.
    pub async fn status(&self) -> eyre::Result<JobStatus> {
        Ok(self.details().await?.status())
    }

    /// Get the cached details of the job, if available.
    ///
    /// This is useful for getting the details of a job without
    /// making a network request.
    pub fn details_cached(&self) -> Option<Arc<JobDetails>> {
        self.details.lock().as_ref().cloned()
    }

    /// Cancel the job.
    pub async fn cancel(&self) -> eyre::Result<()> {
        self.client
            .clone()
            .cancel_jobs(tonic::Request::new(
                ora_proto::server::v1::CancelJobsRequest {
                    filter: Some(JobFilter::new().with_job_id(self.id).into()),
                },
            ))
            .await
            .wrap_err("failed to cancel job")?;

        Ok(())
    }

    /// Cast the job handle to a specific job type.
    ///
    /// This does not perform any runtime checks, so it is up to the caller
    /// to ensure that the job type is correct.
    pub fn cast_type<T: JobType>(&self) -> JobHandle<T, C> {
        JobHandle {
            id: self.id,
            client: self.client.clone(),
            details: self.details.clone(),
            _job_type: std::marker::PhantomData,
        }
    }

    /// Cast the job handle to an unknown job type.
    pub fn cast_unknown(&self) -> JobHandle<(), C> {
        JobHandle {
            id: self.id,
            client: self.client.clone(),
            details: self.details.clone(),
            _job_type: std::marker::PhantomData,
        }
    }
}

impl<J, C> JobHandle<J, C>
where
    J: JobType,
    C: tonic::client::GrpcService<tonic::body::BoxBody> + Clone,
    C::Error: Into<StdError>,
    C::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
    <C::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
{
    /// Retrieve the input of the job.
    pub async fn input(&self) -> eyre::Result<J> {
        let details = self.details().await?;
        serde_json::from_str(&details.input_payload_json)
            .wrap_err("failed to deserialize job input")
    }

    /// Retrieve the output of the job.
    pub async fn wait(&self) -> eyre::Result<J::Output> {
        self.wait_with_interval(std::time::Duration::from_millis(100))
            .await
    }

    /// Retrieve the output of the job with a given polling interval.
    pub async fn wait_with_interval(
        &self,
        interval: std::time::Duration,
    ) -> eyre::Result<J::Output> {
        loop {
            let details = self.details().await?;

            if details.active {
                tokio::time::sleep(interval).await;
                continue;
            }

            let exec = details
                .executions
                .last()
                .ok_or_eyre("no executions found for job")?;

            if let Some(failed) = &exec.failure_reason {
                return Err(eyre::eyre!("job failed: {failed}"));
            }

            if let Some(output) = &exec.output_payload_json {
                return serde_json::from_str(output).wrap_err("failed to deserialize job output");
            }

            bail!("job has no output or failure reason, but it is not active");
        }
    }
}

impl<J, C> IntoFuture for JobHandle<J, C>
where
    J: JobType,
    C: tonic::client::GrpcService<tonic::body::BoxBody> + Clone + Send + Sync + 'static,
    <C as tonic::client::GrpcService<tonic::body::BoxBody>>::Future: Send,
    C::Error: Into<StdError> + Send,
    C::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
    <C::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
{
    type Output = eyre::Result<J::Output>;

    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        async move { self.wait().await }.boxed()
    }
}
