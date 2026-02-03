use std::time::SystemTime;

use crate::{
    grpc::GrpcImpl,
    proto::admin::v1::{self, admin_service_server::AdminService},
    util::{deduplicate_labels, inherit_labels, validate_json},
};
use ora_backend::{
    Backend,
    common::NextPageToken,
    jobs::{JobDefinition, JobFilters, NewJob},
};
use tonic::{Request, Response, Status, async_trait};

use crate::grpc::conv::ResultErrExt;

#[async_trait]
impl<B> AdminService for GrpcImpl<B>
where
    B: Backend,
{
    async fn list_job_types(
        &self,
        _request: Request<v1::ListJobTypesRequest>,
    ) -> Result<Response<v1::ListJobTypesResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let mut job_types = self.executor_pool.list_job_types();
        job_types.extend(self.backend.list_job_types().await.err_status()?);

        job_types.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        job_types.dedup_by(|a, b| a.id.as_str() == b.id.as_str());

        Ok(Response::new(v1::ListJobTypesResponse {
            job_types: job_types.into_iter().map(Into::into).collect(),
        }))
    }

    async fn add_jobs(
        &self,
        request: Request<v1::AddJobsRequest>,
    ) -> Result<Response<v1::AddJobsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let mut jobs: Vec<JobDefinition> = request
            .jobs
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        for job in &mut jobs {
            deduplicate_labels(&mut job.labels);

            validate_json(&job.input_payload_json).map_err(|e| {
                Status::invalid_argument(format!(
                    "invalid JSON payload for job '{}': {e}",
                    job.job_type_id
                ))
            })?;
        }

        let if_not_exists = match request.if_not_exists {
            Some(f) => Some(f.try_into().err_status()?),
            None => None,
        };

        let job_ids = self
            .backend
            .add_jobs(
                &jobs
                    .into_iter()
                    .map(|job| NewJob {
                        job,
                        schedule_id: None,
                    })
                    .collect::<Vec<_>>(),
                if_not_exists,
            )
            .await
            .err_status()?;

        Ok(Response::new(v1::AddJobsResponse {
            job_ids: job_ids.into_iter().map(|id| id.0.to_string()).collect(),
        }))
    }

    async fn list_jobs(
        &self,
        request: Request<v1::ListJobsRequest>,
    ) -> Result<Response<v1::ListJobsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let order_by = request.order_by().try_into()?;
        let filters = request.filters.unwrap_or_default();
        let pagination = request.pagination.unwrap_or_default();

        let (jobs, next_page_token) = self
            .backend
            .list_jobs(
                filters.try_into()?,
                order_by,
                pagination.page_size,
                pagination.next_page_token.map(NextPageToken),
            )
            .await
            .err_status()?;

        Ok(Response::new(v1::ListJobsResponse {
            jobs: jobs.into_iter().map(Into::into).collect(),
            next_page_token: next_page_token.map(|t| t.0),
        }))
    }

    async fn count_jobs(
        &self,
        request: Request<v1::CountJobsRequest>,
    ) -> Result<Response<v1::CountJobsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let filters = request.filters.unwrap_or_default();

        let count = self
            .backend
            .count_jobs(filters.try_into()?)
            .await
            .err_status()?;

        Ok(Response::new(v1::CountJobsResponse { count }))
    }

    async fn cancel_jobs(
        &self,
        request: Request<v1::CancelJobsRequest>,
    ) -> Result<Response<v1::CancelJobsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let filters = request.filters.unwrap_or_default();

        let cancelled_jobs = self
            .backend
            .cancel_jobs(filters.try_into()?)
            .await
            .err_status()?;

        self.executor_pool.cancel_executions(&cancelled_jobs);

        Ok(Response::new(v1::CancelJobsResponse {
            cancelled_job_ids: cancelled_jobs
                .into_iter()
                .map(|j| j.job_id.to_string())
                .collect(),
        }))
    }

    async fn add_schedules(
        &self,
        request: Request<v1::AddSchedulesRequest>,
    ) -> Result<Response<v1::AddSchedulesResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let if_not_exists = match request.if_not_exists {
            Some(f) => Some(f.try_into().err_status()?),
            None => None,
        };

        let mut schedules: Vec<ora_backend::schedules::ScheduleDefinition> = request
            .schedules
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        for schedule in &mut schedules {
            deduplicate_labels(&mut schedule.labels);
        }

        if request.inherit_labels.unwrap_or(true) {
            for schedule in &mut schedules {
                inherit_labels(&schedule.labels, &mut schedule.job_template.labels);
            }
        }

        let schedule_ids = self
            .backend
            .add_schedules(&schedules, if_not_exists)
            .await
            .err_status()?;

        Ok(Response::new(v1::AddSchedulesResponse {
            schedule_ids: schedule_ids
                .into_iter()
                .map(|id| id.0.to_string())
                .collect(),
        }))
    }

    async fn list_schedules(
        &self,
        request: Request<v1::ListSchedulesRequest>,
    ) -> Result<Response<v1::ListSchedulesResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let order_by = request.order_by().try_into()?;
        let filters = request.filters.unwrap_or_default();
        let pagination = request.pagination.unwrap_or_default();

        let (schedules, next_page_token) = self
            .backend
            .list_schedules(
                filters.try_into()?,
                order_by,
                pagination.page_size,
                pagination.next_page_token.map(NextPageToken),
            )
            .await
            .err_status()?;

        Ok(Response::new(v1::ListSchedulesResponse {
            schedules: schedules.into_iter().map(Into::into).collect(),
            next_page_token: next_page_token.map(|t| t.0),
        }))
    }

    async fn count_schedules(
        &self,
        request: Request<v1::CountSchedulesRequest>,
    ) -> Result<Response<v1::CountSchedulesResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let filters = request.filters.unwrap_or_default();

        let count = self
            .backend
            .count_schedules(filters.try_into()?)
            .await
            .err_status()?;

        Ok(Response::new(v1::CountSchedulesResponse { count }))
    }

    async fn stop_schedules(
        &self,
        request: Request<v1::StopSchedulesRequest>,
    ) -> Result<Response<v1::StopSchedulesResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let filters = request.filters.unwrap_or_default();

        let stopped_schedules = self
            .backend
            .stop_schedules(filters.try_into()?)
            .await
            .err_status()?;

        if request.cancel_active_jobs {
            self.backend
                .cancel_jobs(JobFilters {
                    schedule_ids: Some(stopped_schedules.iter().map(|s| s.schedule_id).collect()),
                    ..Default::default()
                })
                .await
                .err_status()?;
        }

        Ok(Response::new(v1::StopSchedulesResponse {
            cancelled_schedule_ids: stopped_schedules
                .into_iter()
                .map(|s| s.schedule_id.to_string())
                .collect(),
        }))
    }

    async fn list_executors(
        &self,
        _request: Request<v1::ListExecutorsRequest>,
    ) -> Result<Response<v1::ListExecutorsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        Ok(Response::new(v1::ListExecutorsResponse {
            executors: self.executor_pool.list_executors(),
        }))
    }

    async fn delete_historical_data(
        &self,
        request: tonic::Request<v1::DeleteHistoricalDataRequest>,
    ) -> std::result::Result<tonic::Response<v1::DeleteHistoricalDataResponse>, tonic::Status> {
        if self.wg.is_waiting() {
            return Err(tonic::Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let before = request
            .before
            .map(TryInto::try_into)
            .transpose()
            .err_status()?;

        self.backend
            .delete_history(before.unwrap_or_else(SystemTime::now))
            .await
            .err_status()?;

        Ok(tonic::Response::new(v1::DeleteHistoricalDataResponse {}))
    }
}
