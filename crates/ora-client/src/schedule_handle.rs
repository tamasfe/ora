//! A handle to a schedule.

use std::sync::Arc;

use futures::Stream;
use ora_proto::server::v1::admin_service_client::AdminServiceClient;
use parking_lot::Mutex;
use tonic::transport::Channel;
use uuid::Uuid;

use crate::{
    admin::AdminClientError,
    job_handle::JobHandle,
    job_query::{JobFilter, JobOrder},
    schedule_definition::ScheduleDetails,
    schedule_query::ScheduleFilter,
    AdminClient,
};
#[allow(clippy::wildcard_imports)]
use tonic::codegen::*;

/// A handle to a schedule.
#[derive(Debug, Clone)]
pub struct ScheduleHandle<C = Channel> {
    /// The ID of the schedule.
    id: Uuid,
    /// The client used to interact with the schedule.
    client: AdminServiceClient<C>,
    details: Arc<Mutex<Option<Arc<ScheduleDetails>>>>,
}

impl<C> ScheduleHandle<C>
where
    C: tonic::client::GrpcService<tonic::body::BoxBody> + Clone + Send + Sync + 'static,
    <C as tonic::client::GrpcService<tonic::body::BoxBody>>::Future: Send,
    C::Error: Into<StdError>,
    C::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
    <C::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
{
    /// Create a new schedule handle.
    pub(crate) fn new(id: Uuid, client: AdminServiceClient<C>) -> Self {
        Self {
            id,
            client,
            details: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the ID of the schedule.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get the details of the schedule.
    pub async fn details(&self) -> eyre::Result<Arc<ScheduleDetails>> {
        if let Some(details) = self.details.lock().as_ref() {
            if !details.active {
                return Ok(details.clone());
            }
        }

        let schedule = self
            .client
            .clone()
            .list_schedules(tonic::Request::new(
                ora_proto::server::v1::ListSchedulesRequest {
                    cursor: None,
                    limit: 1,
                    order: None,
                    filter: Some(ScheduleFilter::new().with_schedule_id(self.id).into()),
                },
            ))
            .await?
            .into_inner()
            .schedules
            .into_iter()
            .next()
            .ok_or_else(|| eyre::eyre!("schedule details not found"))?;

        let details = Arc::new(ScheduleDetails::try_from(schedule)?);
        self.set_details(details.clone());

        Ok(details)
    }

    /// Get the cached details of the schedule, if available.
    pub fn cached_details(&self) -> Option<Arc<ScheduleDetails>> {
        self.details.lock().as_ref().cloned()
    }

    /// Get the jobs that were created by this schedule with the given filter and order.
    pub fn jobs(
        &self,
        mut filter: JobFilter,
        order: JobOrder,
    ) -> impl Stream<Item = Result<JobHandle<(), C>, AdminClientError>> + Send + Unpin + 'static
    {
        filter.schedule_ids = [self.id].into_iter().collect();
        AdminClient::new(self.client.clone()).jobs(filter, order)
    }

    /// Return the amount of jobs that were created by this schedule.
    pub async fn job_count(&self) -> Result<u64, AdminClientError> {
        let count = self
            .client
            .clone()
            .count_jobs(tonic::Request::new(
                ora_proto::server::v1::CountJobsRequest {
                    filter: Some(JobFilter::new().with_schedule_id(self.id).into()),
                },
            ))
            .await?
            .into_inner()
            .count;

        Ok(count)
    }

    /// Get the active job of the schedule.
    pub async fn active_job(&self) -> Result<Option<JobHandle<(), C>>, AdminClientError> {
        let job = self
            .client
            .clone()
            .list_jobs(tonic::Request::new(
                ora_proto::server::v1::ListJobsRequest {
                    cursor: None,
                    limit: 1,
                    order: None,
                    filter: Some(
                        JobFilter::new()
                            .with_schedule_id(self.id)
                            .active_only()
                            .into(),
                    ),
                },
            ))
            .await?
            .into_inner()
            .jobs
            .into_iter()
            .next();

        Ok(job
            .map(|job| {
                let h = JobHandle::new(job.id.parse()?, self.client.clone());
                h.set_details(Arc::new(job.try_into()?));
                Result::<_, eyre::Report>::Ok(h)
            })
            .transpose()?)
    }

    /// Cancel the schedule, optionally cancelling all jobs created by the schedule.
    pub async fn cancel(&self, cancel_jobs: bool) -> Result<(), AdminClientError> {
        self.client
            .clone()
            .cancel_schedules(tonic::Request::new(
                ora_proto::server::v1::CancelSchedulesRequest {
                    filter: Some(ScheduleFilter::new().with_schedule_id(self.id).into()),
                    cancel_jobs,
                },
            ))
            .await?;

        Ok(())
    }

    pub(crate) fn set_details(&self, details: Arc<ScheduleDetails>) {
        *self.details.lock() = Some(details);
    }
}
