use std::time::SystemTime;

use eyre::Context;
use ora_proto::{
    common::v1::{
        schedule_job_creation_policy::JobCreation, schedule_job_timing_policy, JobDefinition,
        ScheduleDefinition,
    },
    server::v1::{
        self, admin_service_server::AdminService, AddJobsRequest, AddJobsResponse,
        CancelJobsRequest, CancelJobsResponse, CountJobsRequest, CountJobsResponse,
        CreateSchedulesResponse, ListJobTypesRequest, ListJobTypesResponse, ListJobsRequest,
        ListJobsResponse, ListSchedulesResponse,
    },
};
use tonic::{async_trait, Request, Response, Status};
use uuid::Uuid;
use wgroup::WaitGroupHandle;

use crate::{events::EventBus, executor_registry::ExecutorRegistry};

use ora_storage::{
    JobQueryFilters, NewJob, NewSchedule, ScheduleJobCreationPolicy, ScheduleJobTimingPolicy,
    ScheduleNewJobDefinition, ScheduleQueryFilters, ScheduleTimeRange, SchedulingPolicyCron,
    SchedulingPolicyRepeat, Storage,
};

#[derive(Debug, Clone)]
pub(crate) struct Admin<S> {
    storage: S,
    wg: WaitGroupHandle,
    event_bus: EventBus,
    executor_registry: ExecutorRegistry<S>,
}

impl<S> Admin<S> {
    /// Create a new admin service with the given storage backend.
    pub(crate) fn new(
        backend: S,
        wg: WaitGroupHandle,
        event_bus: EventBus,
        executor_registry: ExecutorRegistry<S>,
    ) -> Self {
        Self {
            storage: backend,
            wg,
            event_bus,
            executor_registry,
        }
    }
}

#[async_trait]
impl<S> AdminService for Admin<S>
where
    S: Storage,
{
    async fn add_jobs(
        &self,
        request: Request<AddJobsRequest>,
    ) -> Result<Response<AddJobsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        if request.jobs.is_empty() {
            return Ok(Response::new(AddJobsResponse {
                job_ids: Vec::new(),
            }));
        }

        let job_ids = create_jobs(&self.storage, request.jobs)
            .await
            .map_err(|error| {
                tracing::error!(?error, "failed to create jobs");
                error
                    .downcast()
                    .unwrap_or_else(|_| Status::internal("internal error"))
            })?;

        self.event_bus
            .emit_job_event(crate::events::JobEvent::JobsCreated);

        Ok(Response::new(AddJobsResponse {
            job_ids: job_ids.into_iter().map(Into::into).collect(),
        }))
    }

    async fn list_jobs(
        &self,
        request: Request<ListJobsRequest>,
    ) -> Result<Response<ListJobsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let order = request.order().into();

        let limit = if request.limit == 0 {
            100_usize
        } else {
            usize::try_from(request.limit).unwrap_or(100)
        };

        let filter = if let Some(filter) = request.filter {
            JobQueryFilters::try_from(filter)?
        } else {
            JobQueryFilters::default()
        };

        let result = self
            .storage
            .query_jobs(request.cursor, limit, order, filter)
            .await
            .map_err(|err| Status::internal(err.to_string()))?;

        Ok(Response::new(ListJobsResponse {
            jobs: result.jobs.into_iter().map(Into::into).collect(),
            cursor: result.cursor,
            has_more: result.has_more,
        }))
    }

    async fn count_jobs(
        &self,
        request: Request<CountJobsRequest>,
    ) -> Result<Response<CountJobsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let filter = if let Some(filter) = request.filter {
            JobQueryFilters::try_from(filter)?
        } else {
            JobQueryFilters::default()
        };

        let count = self
            .storage
            .count_jobs(filter)
            .await
            .map_err(|err| Status::internal(err.to_string()))?;

        Ok(Response::new(CountJobsResponse { count }))
    }

    async fn list_job_types(
        &self,
        _request: Request<ListJobTypesRequest>,
    ) -> Result<Response<ListJobTypesResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        Ok(Response::new(ListJobTypesResponse {
            job_types: self
                .storage
                .query_job_types()
                .await
                .map_err(|err| Status::internal(err.to_string()))?
                .into_iter()
                .map(Into::into)
                .collect(),
        }))
    }

    async fn cancel_jobs(
        &self,
        request: Request<CancelJobsRequest>,
    ) -> Result<Response<CancelJobsResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let filter = if let Some(filter) = request.filter {
            JobQueryFilters::try_from(filter)?
        } else {
            JobQueryFilters::default()
        };

        let cancelled_jobs = cancel_jobs(&self.storage, &self.executor_registry, filter)
            .await
            .map_err(|error| {
                tracing::error!(?error, "failed to cancel jobs");
                error
                    .downcast()
                    .unwrap_or_else(|_| Status::internal("internal error"))
            })?;

        Ok(Response::new(CancelJobsResponse {
            job_ids: cancelled_jobs
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
        }))
    }

    async fn delete_inactive_jobs(
        &self,
        request: tonic::Request<v1::DeleteInactiveJobsRequest>,
    ) -> std::result::Result<tonic::Response<v1::DeleteInactiveJobsResponse>, tonic::Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let mut filter = if let Some(filter) = request.filter {
            JobQueryFilters::try_from(filter)?
        } else {
            JobQueryFilters::default()
        };

        // We only want to delete inactive jobs.
        filter.active = Some(false);

        let job_ids = self.storage.delete_jobs(filter).await.map_err(|error| {
            tracing::error!(?error, "failed to delete jobs");
            Status::internal("internal error")
        })?;

        Ok(Response::new(v1::DeleteInactiveJobsResponse {
            job_ids: job_ids.into_iter().map(|id| id.to_string()).collect(),
        }))
    }

    async fn list_executors(
        &self,
        _request: tonic::Request<v1::ListExecutorsRequest>,
    ) -> std::result::Result<tonic::Response<v1::ListExecutorsResponse>, tonic::Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        Ok(Response::new(v1::ListExecutorsResponse {
            executors: self.executor_registry.executor_info(),
        }))
    }

    async fn create_schedules(
        &self,
        request: tonic::Request<v1::CreateSchedulesRequest>,
    ) -> std::result::Result<tonic::Response<CreateSchedulesResponse>, tonic::Status> {
        if self.wg.is_waiting() {
            return Err(tonic::Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        if request.schedules.is_empty() {
            return Ok(tonic::Response::new(CreateSchedulesResponse {
                schedule_ids: Vec::new(),
            }));
        }

        let schedule_ids = create_schedules(&self.storage, request.schedules)
            .await
            .map_err(|error| {
                tracing::error!(?error, "failed to create schedules");
                error
                    .downcast()
                    .unwrap_or_else(|_| tonic::Status::internal("internal error"))
            })?;

        self.event_bus
            .emit_schedule_event(crate::events::ScheduleEvent::SchedulesAdded);

        Ok(Response::new(CreateSchedulesResponse {
            schedule_ids: schedule_ids.into_iter().map(Into::into).collect(),
        }))
    }

    async fn list_schedules(
        &self,
        request: tonic::Request<v1::ListSchedulesRequest>,
    ) -> std::result::Result<tonic::Response<v1::ListSchedulesResponse>, tonic::Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let order = request.order().into();

        let limit = if request.limit == 0 {
            100_usize
        } else {
            usize::try_from(request.limit).unwrap_or(100)
        };

        let filter = if let Some(filter) = request.filter {
            ScheduleQueryFilters::try_from(filter)?
        } else {
            ScheduleQueryFilters::default()
        };

        let result = self
            .storage
            .query_schedules(request.cursor, limit, filter, order)
            .await
            .map_err(|err| Status::internal(err.to_string()))?;

        Ok(Response::new(ListSchedulesResponse {
            schedules: result.schedules.into_iter().map(Into::into).collect(),
            cursor: result.cursor,
            has_more: result.has_more,
        }))
    }

    async fn count_schedules(
        &self,
        request: tonic::Request<v1::CountSchedulesRequest>,
    ) -> std::result::Result<tonic::Response<v1::CountSchedulesResponse>, tonic::Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let filter = if let Some(filter) = request.filter {
            ScheduleQueryFilters::try_from(filter)?
        } else {
            ScheduleQueryFilters::default()
        };

        let count = self
            .storage
            .count_schedules(filter)
            .await
            .map_err(|err| Status::internal(err.to_string()))?;

        Ok(Response::new(v1::CountSchedulesResponse { count }))
    }

    async fn cancel_schedules(
        &self,
        request: tonic::Request<v1::CancelSchedulesRequest>,
    ) -> std::result::Result<tonic::Response<v1::CancelSchedulesResponse>, tonic::Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let mut filter = if let Some(filter) = request.filter {
            ScheduleQueryFilters::try_from(filter)?
        } else {
            ScheduleQueryFilters::default()
        };

        // We cannot cancel inactive schedules.
        filter.active = Some(true);

        let schedule_ids = self
            .storage
            .query_schedule_ids(filter)
            .await
            .map_err(|err| Status::internal(err.to_string()))?;

        if schedule_ids.is_empty() {
            return Ok(Response::new(v1::CancelSchedulesResponse {
                schedule_ids: Vec::new(),
                job_ids: Vec::new(),
            }));
        }

        let now = SystemTime::now();

        let cancelled_schedules = self
            .storage
            .schedules_cancelled(&schedule_ids, now)
            .await
            .map_err(|error| {
                tracing::error!(?error, "failed to cancel schedules");
                Status::internal("internal error")
            })?;

        // We also need to cancel any active jobs of all cancelled schedules.
        let cancelled_jobs = cancel_jobs(
            &self.storage,
            &self.executor_registry,
            JobQueryFilters {
                active: Some(true),
                schedule_ids: Some(cancelled_schedules.iter().map(|s| s.id).collect()),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| {
            tracing::error!(?error, "failed to cancel jobs");
            error
                .downcast()
                .unwrap_or_else(|_| Status::internal("internal error"))
        })?;

        Ok(Response::new(v1::CancelSchedulesResponse {
            schedule_ids: cancelled_schedules
                .into_iter()
                .map(|schedule| schedule.id.to_string())
                .collect(),
            job_ids: cancelled_jobs
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
        }))
    }

    async fn delete_inactive_schedules(
        &self,
        request: tonic::Request<v1::DeleteInactiveSchedulesRequest>,
    ) -> std::result::Result<tonic::Response<v1::DeleteInactiveSchedulesResponse>, tonic::Status>
    {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let request = request.into_inner();

        let mut filter = if let Some(filter) = request.filter {
            ScheduleQueryFilters::try_from(filter)?
        } else {
            ScheduleQueryFilters::default()
        };

        // We only want to delete inactive schedules.
        filter.active = Some(false);

        let schedule_ids = self
            .storage
            .delete_schedules(filter)
            .await
            .map_err(|error| {
                tracing::error!(?error, "failed to delete schedules");
                Status::internal("internal error")
            })?;

        Ok(Response::new(v1::DeleteInactiveSchedulesResponse {
            schedule_ids: schedule_ids.into_iter().map(|id| id.to_string()).collect(),
        }))
    }
}

/// Create jobs from the given definitions.
#[allow(clippy::cast_possible_truncation)]
async fn create_jobs(
    backend: &impl Storage,
    definitions: Vec<JobDefinition>,
) -> eyre::Result<Vec<Uuid>> {
    let now = SystemTime::now();

    let mut new_jobs = Vec::<NewJob>::with_capacity(definitions.len());
    let mut new_job_ids = Vec::with_capacity(definitions.len());

    for definition in definitions {
        let job_id = Uuid::now_v7();
        new_job_ids.push(job_id);

        let job = NewJob {
            created_at: now,
            id: job_id,
            schedule_id: None,
            job_type_id: definition.job_type_id,
            input_payload_json: definition.input_payload_json,
            target_execution_time: definition
                .target_execution_time
                .and_then(|t| SystemTime::try_from(t).ok())
                .unwrap_or(now),
            retry_policy: definition.retry_policy.map(Into::into).unwrap_or_default(),
            timeout_policy: definition
                .timeout_policy
                .map(Into::into)
                .unwrap_or_default(),
            labels: definition
                .labels
                .into_iter()
                .map(|label| (label.key, label.value))
                .collect(),
            metadata_json: definition.metadata_json,
        };
        new_jobs.push(job);
    }

    backend
        .jobs_added(new_jobs)
        .await
        .wrap_err("failed to persist jobs")?;

    Ok(new_job_ids)
}

async fn create_schedules(
    backend: &impl Storage,
    schedules: Vec<ScheduleDefinition>,
) -> eyre::Result<Vec<Uuid>> {
    let now = SystemTime::now();

    let mut new_schedules = Vec::<NewSchedule>::with_capacity(schedules.len());
    let mut new_schedule_ids = Vec::with_capacity(schedules.len());

    for schedule in schedules {
        let schedule_id = Uuid::now_v7();
        new_schedule_ids.push(schedule_id);

        let new_schedule = NewSchedule {
            created_at: now,
            id: schedule_id,
            labels: schedule
                .labels
                .into_iter()
                .map(|label| (label.key, label.value))
                .collect(),
            job_timing_policy: {
                let job_timing = schedule
                    .job_timing_policy
                    .ok_or_else(|| Status::invalid_argument("missing job timing policy"))?
                    .job_timing
                    .ok_or_else(|| Status::invalid_argument("missing job timing policy"))?;

                match job_timing {
                    schedule_job_timing_policy::JobTiming::Repeat(
                        schedule_job_timing_policy_repeat,
                    ) => ScheduleJobTimingPolicy::Repeat(SchedulingPolicyRepeat {
                        interval: schedule_job_timing_policy_repeat
                            .interval
                            .ok_or_else(|| {
                                Status::invalid_argument("missing interval in repeat policy")
                            })?
                            .try_into()
                            .wrap_err("invalid duration")?,

                        immediate: schedule_job_timing_policy_repeat.immediate,
                        missed_policy: schedule_job_timing_policy_repeat
                            .missed_time_policy()
                            .into(),
                    }),
                    schedule_job_timing_policy::JobTiming::Cron(
                        schedule_job_timing_policy_cron,
                    ) => ScheduleJobTimingPolicy::Cron(SchedulingPolicyCron {
                        missed_policy: schedule_job_timing_policy_cron.missed_time_policy().into(),
                        immediate: schedule_job_timing_policy_cron.immediate,
                        cron_expression: {
                            // Validate the cron expression.
                            cronexpr::parse_crontab(
                                &schedule_job_timing_policy_cron.cron_expression,
                            )
                            .map_err(|err| {
                                Status::invalid_argument(format!("invalid cron expression: {err}"))
                            })?;

                            schedule_job_timing_policy_cron.cron_expression
                        },
                    }),
                }
            },
            job_creation_policy: {
                let policy = schedule
                    .job_creation_policy
                    .ok_or_else(|| Status::invalid_argument("missing job creation policy"))?
                    .job_creation
                    .ok_or_else(|| Status::invalid_argument("missing job creation policy"))?;

                match policy {
                    JobCreation::JobDefinition(job_definition) => {
                        ScheduleJobCreationPolicy::JobDefinition(ScheduleNewJobDefinition {
                            job_type_id: job_definition.job_type_id,
                            input_payload_json: job_definition.input_payload_json,
                            timeout_policy: job_definition
                                .timeout_policy
                                .map(Into::into)
                                .unwrap_or_default(),
                            retry_policy: job_definition
                                .retry_policy
                                .map(Into::into)
                                .unwrap_or_default(),
                            labels: job_definition
                                .labels
                                .into_iter()
                                .map(|label| (label.key, label.value))
                                .collect(),
                        })
                    }
                }
            },
            time_range: schedule
                .time_range
                .map(|range| {
                    let range = ScheduleTimeRange {
                        start: range.start.map(SystemTime::try_from).transpose().map_err(
                            |error| {
                                Status::invalid_argument(format!("unsupported timestamp: {error}"))
                            },
                        )?,
                        end: range
                            .end
                            .map(SystemTime::try_from)
                            .transpose()
                            .map_err(|error| {
                                Status::invalid_argument(format!("unsupported timestamp: {error}"))
                            })?,
                    };

                    if !range.is_valid() {
                        return Err(Status::invalid_argument("invalid time range"));
                    }

                    Ok(range)
                })
                .transpose()?,
            metadata_json: schedule.metadata_json,
        };
        new_schedules.push(new_schedule);
    }

    backend
        .schedules_added(new_schedules)
        .await
        .wrap_err("failed to persist schedules")?;

    Ok(new_schedule_ids)
}

async fn cancel_jobs(
    storage: &impl Storage,
    executor_registry: &ExecutorRegistry<impl Storage>,
    mut filter: JobQueryFilters,
) -> eyre::Result<Vec<Uuid>> {
    // We cannot cancel inactive jobs.
    filter.active = Some(true);

    let job_ids = storage
        .query_job_ids(filter)
        .await
        .map_err(|err| Status::internal(err.to_string()))?;

    if job_ids.is_empty() {
        return Ok(Vec::new());
    }

    let now = SystemTime::now();

    let cancelled_jobs = storage
        .jobs_cancelled(&job_ids, now)
        .await
        .map_err(|error| {
            tracing::error!(?error, "failed to cancel jobs");
            Status::internal("internal error")
        })?;

    // Fail and cancel any active executions.
    let execution_ids = cancelled_jobs
        .iter()
        .filter_map(|job| job.active_execution)
        .collect::<Vec<_>>();

    if !execution_ids.is_empty() {
        storage
            .executions_failed(&execution_ids, now, "job cancelled".to_string(), true)
            .await
            .map_err(|error| {
                tracing::error!(?error, "failed to fail executions");
                Status::internal("internal error")
            })?;
    }

    executor_registry.cancel_executions(&execution_ids);

    Ok(cancelled_jobs.into_iter().map(|job| job.id).collect())
}
