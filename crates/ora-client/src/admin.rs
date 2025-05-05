//! Wrappers over client connections that provide higher-level abstractions.

use std::sync::Arc;

use futures::{Stream, StreamExt};
use ora_proto::server::v1::{
    admin_service_client::AdminServiceClient, AddJobIfNotExistsRequest, AddJobsRequest,
    AddScheduleIfNotExistsRequest, CountJobsRequest, JobQueryOrder, ListJobsRequest,
    ListSchedulesRequest, ScheduleQueryOrder,
};
use thiserror::Error;
use tonic::{transport::Channel, Request};
use uuid::Uuid;

use crate::{
    job_definition::JobDetails,
    job_handle::JobHandle,
    job_query::{JobFilter, JobOrder},
    job_type::TypedJobDefinition,
    schedule_definition::{ScheduleDefinition, ScheduleDetails},
    schedule_handle::ScheduleHandle,
    schedule_query::{ScheduleFilter, ScheduleOrder},
    JobType,
};

#[allow(clippy::wildcard_imports)]
use tonic::codegen::*;

/// A high-level client for interacting with the Ora server admin endpoints.
#[derive(Debug, Clone)]
#[must_use]
pub struct AdminClient<C = Channel> {
    client: AdminServiceClient<C>,
}

impl<C> AdminClient<C>
where
    C: tonic::client::GrpcService<tonic::body::BoxBody> + Clone + Send + Sync + 'static,
    <C as tonic::client::GrpcService<tonic::body::BoxBody>>::Future: Send,
    C::Error: Into<StdError>,
    C::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
    <C::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
{
    /// Create a new admin client from a gRPC client.
    pub fn new(client: AdminServiceClient<C>) -> Self {
        Self { client }
    }

    /// Add a new job to be executed.
    pub async fn add_job<J>(
        &self,
        definition: TypedJobDefinition<J>,
    ) -> Result<JobHandle<J, C>, AdminClientError> {
        let res = self
            .client
            .clone()
            .add_jobs(Request::new(AddJobsRequest {
                jobs: vec![definition.into_inner().into()],
            }))
            .await?
            .into_inner();

        let job_id: Uuid = res
            .job_ids
            .into_iter()
            .next()
            .ok_or(AdminClientError::NotEnoughJobIds {
                expected: 1,
                actual: 0,
            })?
            .parse()?;

        Ok(JobHandle::new(job_id, self.client.clone()))
    }

    /// Add a new job to be executed.
    ///
    /// If an active job with the same job type and
    /// labels already exists, it will be returned instead.
    pub async fn add_job_persistent<J>(
        &self,
        definition: TypedJobDefinition<J>,
    ) -> Result<PersistentJob<J, C>, AdminClientError> {
        let mut filters = JobFilter::default()
            .with_job_type_id(definition.inner.job_type_id.to_string())
            .active_only();

        for (key, value) in &definition.inner.labels {
            filters = filters.with_label_value(key, value);
        }

        let res = self
            .client
            .clone()
            .add_job_if_not_exists(Request::new(AddJobIfNotExistsRequest {
                job: Some(definition.into_inner().into()),
                filter: Some(filters.into()),
            }))
            .await?
            .into_inner();

        let job_id: Uuid = res.job_id.parse()?;

        if res.added {
            Ok(PersistentJob::Added(JobHandle::new(
                job_id,
                self.client.clone(),
            )))
        } else {
            Ok(PersistentJob::Exists(JobHandle::new(
                job_id,
                self.client.clone(),
            )))
        }
    }

    /// Add multiple new jobs to be executed.
    pub async fn add_jobs<J>(
        &self,
        definitions: impl IntoIterator<Item = TypedJobDefinition<J>>,
    ) -> Result<Vec<JobHandle<J, C>>, AdminClientError> {
        let definitions = definitions
            .into_iter()
            .map(|d| d.into_inner().into())
            .collect();

        let res = self
            .client
            .clone()
            .add_jobs(Request::new(AddJobsRequest { jobs: definitions }))
            .await?
            .into_inner();

        let job_ids = res
            .job_ids
            .into_iter()
            .map(|id| id.parse())
            .collect::<Result<Vec<Uuid>, _>>()?;

        Ok(job_ids
            .into_iter()
            .map(|id| JobHandle::new(id, self.client.clone()))
            .collect())
    }

    /// Create a job handle for a specific job.
    ///
    /// Note that this method does not check if the job exists.
    pub fn job(&self, job_id: Uuid) -> JobHandle<(), C> {
        JobHandle::new(job_id, self.client.clone())
    }

    /// Retrieve a list of jobs of a specific type
    /// with a specific filter and order.
    ///
    /// The returned job handles will have their details pre-fetched.
    pub fn jobs_of_type<J>(
        &self,
        mut filter: JobFilter,
        order_by: JobOrder,
    ) -> impl Stream<Item = Result<JobHandle<J, C>, AdminClientError>> + Send + Unpin + 'static
    where
        J: JobType,
    {
        filter.job_type_ids = [J::id().into()].into_iter().collect();

        let client = self.client.clone();

        async_stream::try_stream!({
            let mut cursor: Option<String> = None;

            loop {
                let res = client
                    .clone()
                    .list_jobs(Request::new(ListJobsRequest {
                        cursor: cursor.take(),
                        limit: 100,
                        filter: Some(filter.clone().into()),
                        order: Some(JobQueryOrder::from(order_by) as i32),
                    }))
                    .await?
                    .into_inner();

                for job in res.jobs {
                    let job_id = job.id.parse().map_err(AdminClientError::InvalidId)?;
                    let h = JobHandle::new(job_id, client.clone());
                    h.set_details(Arc::new(JobDetails::try_from(job)?));
                    yield h;
                }

                cursor = res.cursor;

                if !res.has_more {
                    break;
                }
            }
        })
        .boxed()
    }

    /// Retrieve a list of jobs with a specific filter and order.
    ///
    /// The returned job handles will have their details pre-fetched.
    pub fn jobs(
        &self,
        filter: JobFilter,
        order_by: JobOrder,
    ) -> impl Stream<Item = Result<JobHandle<(), C>, AdminClientError>> + Send + Unpin + 'static
    {
        let client = self.client.clone();

        async_stream::try_stream!({
            let mut cursor: Option<String> = None;

            loop {
                let res = client
                    .clone()
                    .list_jobs(Request::new(ListJobsRequest {
                        cursor: cursor.take(),
                        limit: 100,
                        filter: Some(filter.clone().into()),
                        order: Some(JobQueryOrder::from(order_by) as i32),
                    }))
                    .await?
                    .into_inner();

                for job in res.jobs {
                    let job_id = job.id.parse().map_err(AdminClientError::InvalidId)?;
                    let h = JobHandle::new(job_id, client.clone());
                    h.set_details(Arc::new(JobDetails::try_from(job)?));
                    yield h;
                }

                cursor = res.cursor;

                if !res.has_more {
                    break;
                }
            }
        })
        .boxed()
    }

    /// Count the number of jobs with a specific filter.
    pub async fn job_count(&self, filter: JobFilter) -> Result<u64, AdminClientError> {
        let res = self
            .client
            .clone()
            .count_jobs(Request::new(CountJobsRequest {
                filter: Some(filter.into()),
            }))
            .await?;

        Ok(res.into_inner().count)
    }

    /// Count the number of jobs of a specific type with a specific filter.
    pub async fn job_count_of_type<J>(&self, mut filter: JobFilter) -> Result<u64, AdminClientError>
    where
        J: JobType,
    {
        filter.job_type_ids = [J::id().into()].into_iter().collect();

        let res = self
            .client
            .clone()
            .count_jobs(Request::new(CountJobsRequest {
                filter: Some(filter.into()),
            }))
            .await?;

        Ok(res.into_inner().count)
    }

    /// Cancel jobs with a specific filter.
    ///
    /// Returns handles to jobs that were successfully cancelled.
    pub async fn cancel_jobs(
        &self,
        filter: JobFilter,
    ) -> Result<Vec<JobHandle<(), C>>, AdminClientError> {
        let res = self
            .client
            .clone()
            .cancel_jobs(Request::new(ora_proto::server::v1::CancelJobsRequest {
                filter: Some(filter.into()),
            }))
            .await?
            .into_inner();

        let job_ids = res
            .job_ids
            .into_iter()
            .map(|id| id.parse())
            .collect::<Result<Vec<Uuid>, _>>()?;

        Ok(job_ids
            .into_iter()
            .map(|id| JobHandle::new(id, self.client.clone()))
            .collect())
    }

    /// Delete inactive jobs and associated with a specific filter,
    /// returning the deleted job IDs.
    pub async fn delete_inactive_jobs(
        &self,
        filter: JobFilter,
    ) -> Result<Vec<Uuid>, AdminClientError> {
        let res = self
            .client
            .clone()
            .delete_inactive_jobs(Request::new(
                ora_proto::server::v1::DeleteInactiveJobsRequest {
                    filter: Some(filter.into()),
                },
            ))
            .await?
            .into_inner();

        let job_ids = res
            .job_ids
            .into_iter()
            .map(|id| id.parse())
            .collect::<Result<Vec<Uuid>, _>>()?;

        Ok(job_ids)
    }

    /// Add a new schedule.
    pub async fn add_schedule(
        &self,
        mut schedule: ScheduleDefinition,
    ) -> Result<ScheduleHandle<C>, AdminClientError> {
        if schedule.propagate_labels_to_jobs {
            match &mut schedule.job_creation_policy {
                crate::schedule_definition::ScheduleJobCreationPolicy::JobDefinition(
                    job_definition,
                ) => {
                    for (key, value) in &schedule.labels {
                        if job_definition.labels.contains_key(key) {
                            continue;
                        }

                        job_definition.labels.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        let res = self
            .client
            .clone()
            .add_schedules(Request::new(ora_proto::server::v1::AddSchedulesRequest {
                schedules: vec![schedule.into()],
            }))
            .await?
            .into_inner();

        let schedule_id: Uuid = res
            .schedule_ids
            .into_iter()
            .next()
            .ok_or(AdminClientError::NotEnoughScheduleIds {
                expected: 1,
                actual: 0,
            })?
            .parse()?;

        Ok(ScheduleHandle::new(schedule_id, self.client.clone()))
    }

    /// Add a new schedule.
    ///
    /// If an active schedule with the same job type and
    /// labels already exists, it will be returned instead.
    pub async fn add_schedule_persistent(
        &self,
        mut schedule: ScheduleDefinition,
    ) -> Result<PersistentSchedule<C>, AdminClientError> {
        if schedule.propagate_labels_to_jobs {
            match &mut schedule.job_creation_policy {
                crate::schedule_definition::ScheduleJobCreationPolicy::JobDefinition(
                    job_definition,
                ) => {
                    for (key, value) in &schedule.labels {
                        if job_definition.labels.contains_key(key) {
                            continue;
                        }

                        job_definition.labels.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        let job_type_id = match schedule.job_creation_policy {
            crate::schedule_definition::ScheduleJobCreationPolicy::JobDefinition(
                ref job_definition,
            ) => job_definition.job_type_id.to_string(),
        };

        let mut filters = ScheduleFilter::default()
            .with_job_type_id(job_type_id)
            .active_only();

        for (key, value) in &schedule.labels {
            filters = filters.with_label_value(key, value);
        }

        let res = self
            .client
            .clone()
            .add_schedule_if_not_exists(Request::new(AddScheduleIfNotExistsRequest {
                schedule: Some(schedule.into()),
                filter: Some(filters.into()),
            }))
            .await?
            .into_inner();

        let schedule_id: Uuid = res.schedule_id.parse()?;

        if res.added {
            Ok(PersistentSchedule::Added(ScheduleHandle::new(
                schedule_id,
                self.client.clone(),
            )))
        } else {
            Ok(PersistentSchedule::Exists(ScheduleHandle::new(
                schedule_id,
                self.client.clone(),
            )))
        }
    }

    /// Add multiple new schedules.
    ///
    /// Returns handles to the created schedules.
    pub async fn add_schedules(
        &self,
        schedules: impl IntoIterator<Item = ScheduleDefinition>,
    ) -> Result<Vec<ScheduleHandle<C>>, AdminClientError> {
        let schedules = schedules
            .into_iter()
            .map(|mut schedule| {
                if schedule.propagate_labels_to_jobs {
                    match &mut schedule.job_creation_policy {
                        crate::schedule_definition::ScheduleJobCreationPolicy::JobDefinition(
                            job_definition,
                        ) => {
                            for (key, value) in &schedule.labels {
                                if job_definition.labels.contains_key(key) {
                                    continue;
                                }

                                job_definition.labels.insert(key.clone(), value.clone());
                            }
                        }
                    }
                }

                schedule
            })
            .map(Into::into)
            .collect::<Vec<_>>();

        let res = self
            .client
            .clone()
            .add_schedules(Request::new(ora_proto::server::v1::AddSchedulesRequest {
                schedules,
            }))
            .await?
            .into_inner();

        let schedule_ids = res
            .schedule_ids
            .into_iter()
            .map(|id| id.parse())
            .collect::<Result<Vec<Uuid>, _>>()?;

        Ok(schedule_ids
            .into_iter()
            .map(|id| ScheduleHandle::new(id, self.client.clone()))
            .collect())
    }

    /// Create a new schedule handle for a specific schedule.
    ///
    /// Note that this method does not check if the schedule exists.
    pub fn schedule(&self, schedule_id: Uuid) -> ScheduleHandle<C> {
        ScheduleHandle::new(schedule_id, self.client.clone())
    }

    /// Retrieve a list of schedules with a specific filter and order.
    ///
    /// The returned schedule handles will have their details pre-fetched.
    pub fn schedules(
        &self,
        filter: ScheduleFilter,
        order: ScheduleOrder,
    ) -> impl Stream<Item = Result<ScheduleHandle<C>, AdminClientError>> + Send + Unpin + 'static
    {
        let client = self.client.clone();

        async_stream::try_stream!({
            let mut cursor: Option<String> = None;

            loop {
                let res = client
                    .clone()
                    .list_schedules(Request::new(ListSchedulesRequest {
                        cursor: cursor.take(),
                        limit: 100,
                        filter: Some(filter.clone().into()),
                        order: Some(ScheduleQueryOrder::from(order) as i32),
                    }))
                    .await?
                    .into_inner();

                for schedule in res.schedules {
                    let schedule_id = schedule.id.parse().map_err(AdminClientError::InvalidId)?;
                    let h = ScheduleHandle::new(schedule_id, client.clone());
                    h.set_details(Arc::new(ScheduleDetails::try_from(schedule)?));
                    yield h;
                }

                cursor = res.cursor;

                if !res.has_more {
                    break;
                }
            }
        })
        .boxed()
    }

    /// Return the number of schedules that match a specific filter.
    pub async fn schedule_count(&self, filter: ScheduleFilter) -> Result<u64, AdminClientError> {
        let res = self
            .client
            .clone()
            .count_schedules(Request::new(ora_proto::server::v1::CountSchedulesRequest {
                filter: Some(filter.into()),
            }))
            .await?;

        Ok(res.into_inner().count)
    }

    /// Cancel schedules with a specific filter,
    /// optionally cancelling the jobs associated with them.
    pub async fn cancel_schedules(
        &self,
        filter: ScheduleFilter,
        cancel_jobs: bool,
    ) -> Result<ScheduleCancellationResult<C>, AdminClientError> {
        let res = self
            .client
            .clone()
            .cancel_schedules(Request::new(
                ora_proto::server::v1::CancelSchedulesRequest {
                    filter: Some(filter.into()),
                    cancel_jobs,
                },
            ))
            .await?
            .into_inner();

        let schedule_ids = res
            .schedule_ids
            .into_iter()
            .map(|id| id.parse())
            .collect::<Result<Vec<Uuid>, _>>()?;

        let job_ids = res
            .job_ids
            .into_iter()
            .map(|id| id.parse())
            .collect::<Result<Vec<Uuid>, _>>()?;

        Ok(ScheduleCancellationResult {
            schedules: schedule_ids
                .into_iter()
                .map(|id| ScheduleHandle::new(id, self.client.clone()))
                .collect(),
            jobs: job_ids
                .into_iter()
                .map(|id| JobHandle::new(id, self.client.clone()))
                .collect(),
        })
    }
}

/// The result of a schedule cancellation.
pub struct ScheduleCancellationResult<C> {
    /// Cancelled schedules.
    pub schedules: Vec<ScheduleHandle<C>>,
    /// Cancelled jobs.
    pub jobs: Vec<JobHandle<(), C>>,
}

impl From<AdminServiceClient<Channel>> for AdminClient {
    fn from(client: AdminServiceClient<Channel>) -> Self {
        Self { client }
    }
}

/// The result of a persistent job creation.
pub enum PersistentJob<J, C> {
    /// The job was added successfully.
    Added(JobHandle<J, C>),
    /// The job already exists and is active.
    Exists(JobHandle<J, C>),
}

/// The result of a persistent schedule creation.
pub enum PersistentSchedule<C> {
    /// The schedule was added successfully.
    Added(ScheduleHandle<C>),
    /// The schedule already exists and is active.
    Exists(ScheduleHandle<C>),
}

/// Errors that can occur when interacting with the admin client.
#[allow(missing_docs)]
#[derive(Debug, Error)]
pub enum AdminClientError {
    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),
    #[error("not enough job IDs were returned by the server (expected {expected}, got {actual})")]
    NotEnoughJobIds { expected: usize, actual: usize },
    #[error("invalid ID: {0}")]
    InvalidId(#[from] uuid::Error),
    #[error(
        "not enough schedule IDs were returned by the server (expected {expected}, got {actual})"
    )]
    NotEnoughScheduleIds { expected: usize, actual: usize },
    #[error("{0}")]
    Other(#[from] eyre::Error),
}
