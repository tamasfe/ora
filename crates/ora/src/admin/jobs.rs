//! Job management.

use std::{
    cmp,
    marker::PhantomData,
    pin::{Pin, pin},
    time::SystemTime,
};

use eyre::{Context, ContextCompat, OptionExt, bail};
use futures::{FutureExt, Stream, TryStreamExt};
use tonic::Request;
use uuid::Uuid;

use crate::{
    admin::{AdminClient, schedules::Schedule},
    common::{AddedOrExisting, LabelFilter, TimeRange},
    execution::{ExecutionId, ExecutionStatus},
    executor::ExecutorId,
    job::{JobDefinition, JobId},
    job_type::{AnyJobType, JobType, JobTypeId},
    proto::{
        self,
        admin::v1::{AddJobsRequest, ListJobsRequest, PaginationOptions},
    },
    schedule::ScheduleId,
};

impl AdminClient {
    /// Add a job to be executed.
    pub async fn add_job<J>(&self, job: JobDefinition<J>) -> crate::Result<Job<J>>
    where
        J: JobType,
    {
        let id = self
            .inner
            .add_jobs(Request::new(AddJobsRequest {
                jobs: vec![job.try_into()?],
                if_not_exists: None,
            }))
            .await?
            .into_inner()
            .job_ids
            .pop()
            .ok_or_eyre("job that was just added was not found")?
            .parse::<Uuid>()
            .map(JobId)
            .wrap_err("server returned invalid job ID")?;

        Ok(Job {
            client: self.clone(),
            id,
            phantom: PhantomData,
            raw: None,
        })
    }

    /// Add multiple jobs to be executed.
    pub async fn add_jobs<I>(&self, jobs: I) -> crate::Result<Vec<Job<AnyJobType>>>
    where
        I: IntoIterator<Item: TryInto<proto::jobs::v1::Job, Error: Into<crate::Error>>>,
    {
        let jobs = jobs
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)?;

        self.inner
            .add_jobs(Request::new(AddJobsRequest {
                jobs,
                if_not_exists: None,
            }))
            .await?
            .into_inner()
            .job_ids
            .into_iter()
            .map(|id| {
                Result::<_, crate::Error>::Ok(Job {
                    client: self.clone(),
                    id: JobId(
                        id.parse::<Uuid>()
                            .wrap_err("server returned invalid job ID")?,
                    ),
                    raw: None,
                    phantom: PhantomData,
                })
            })
            .collect::<Result<Vec<_>, _>>()
    }

    /// Add a job to be executed.
    ///
    /// If any jobs exists based on the given filters, no
    /// jobs will be added.
    /// If multiple existing jobs are matched by the filter,
    /// one is arbitrarily chosen and returned.
    pub async fn add_job_if_not_exists<J>(
        &self,
        job: JobDefinition<J>,
        filters: JobFilters,
    ) -> crate::Result<AddedOrExisting<Job<AnyJobType>>>
    where
        J: JobType,
    {
        let res = self
            .inner
            .add_jobs(Request::new(AddJobsRequest {
                jobs: vec![job.try_into()?],
                if_not_exists: Some(filters.into()),
            }))
            .await?
            .into_inner();

        let added_job_id = res
            .job_ids
            .into_iter()
            .next()
            .map(|id| {
                id.parse::<Uuid>()
                    .map(JobId)
                    .wrap_err("server returned invalid job ID")
            })
            .transpose()?;

        let existing_job_id = res
            .existing_job_ids
            .into_iter()
            .next()
            .map(|id| {
                id.parse::<Uuid>()
                    .map(JobId)
                    .wrap_err("server returned invalid job ID")
            })
            .transpose()?;

        match (added_job_id, existing_job_id) {
            (Some(added_job_id), None) => Ok(AddedOrExisting::Added(Job {
                client: self.clone(),
                id: added_job_id,
                raw: None,
                phantom: PhantomData,
            })),
            (None, Some(existing_job_id)) => Ok(AddedOrExisting::Existing(Job {
                client: self.clone(),
                id: existing_job_id,
                raw: None,
                phantom: PhantomData,
            })),
            (None, None) => Err(eyre::eyre!(
                "no job was added but no existing job was returned by the server"
            )
            .into()),
            (Some(_), Some(_)) => Err(eyre::eyre!(
                "server returned both an added job ID and an existing job ID, which is unexpected"
            )
            .into()),
        }
    }

    /// Add multiple jobs if no existing jobs match the given filters.
    pub async fn add_jobs_if_not_exists<I>(
        &self,
        jobs: I,
        filters: JobFilters,
    ) -> crate::Result<AddedOrExisting<Vec<Job<AnyJobType>>>>
    where
        I: IntoIterator<Item: TryInto<proto::jobs::v1::Job, Error = crate::Error>>,
    {
        let jobs = jobs
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        let res = self
            .inner
            .add_jobs(Request::new(AddJobsRequest {
                jobs,
                if_not_exists: Some(filters.into()),
            }))
            .await?
            .into_inner();

        if res.job_ids.is_empty() {
            Ok(AddedOrExisting::Existing(
                res.existing_job_ids
                    .into_iter()
                    .map(|id| {
                        Result::<_, crate::Error>::Ok(Job {
                            client: self.clone(),
                            id: JobId(
                                id.parse::<Uuid>()
                                    .wrap_err("server returned invalid job ID")?,
                            ),
                            raw: None,
                            phantom: PhantomData,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        } else {
            Ok(AddedOrExisting::Added(
                res.job_ids
                    .into_iter()
                    .map(|id| {
                        Result::<_, crate::Error>::Ok(Job {
                            client: self.clone(),
                            id: JobId(
                                id.parse::<Uuid>()
                                    .wrap_err("server returned invalid job ID")?,
                            ),
                            raw: None,
                            phantom: PhantomData,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
    }

    /// List jobs based on the given filters.
    pub fn list_jobs(
        &self,
        filters: JobFilters,
        order: JobOrderBy,
        limit: Option<u32>,
    ) -> impl Stream<Item = crate::Result<Job<AnyJobType>>> {
        async_stream::try_stream!({
            let mut total_count = 0;
            let mut next_page_token = None;

            let filters: proto::admin::v1::JobFilters = filters.into();

            loop {
                if let Some(limit) = limit
                    && total_count >= limit
                {
                    break;
                }

                let response = self
                    .inner
                    .list_jobs(Request::new(ListJobsRequest {
                        filters: Some(filters.clone()),
                        order_by: match order {
                            JobOrderBy::TargetExecutionTimeAsc => {
                                proto::admin::v1::JobOrderBy::TargetExecutionTimeAsc as i32
                            }
                            JobOrderBy::TargetExecutionTimeDesc => {
                                proto::admin::v1::JobOrderBy::TargetExecutionTimeDesc as i32
                            }
                            JobOrderBy::CreatedAtAsc => {
                                proto::admin::v1::JobOrderBy::CreatedAtAsc as i32
                            }
                            JobOrderBy::CreatedAtDesc => {
                                proto::admin::v1::JobOrderBy::CreatedAtDesc as i32
                            }
                        },
                        pagination: Some(PaginationOptions {
                            page_size: if let Some(limit) = limit {
                                cmp::min(25, limit)
                            } else {
                                25
                            },
                            next_page_token: next_page_token.clone(),
                        }),
                    }))
                    .await?
                    .into_inner();

                for job_proto in response.jobs {
                    let job_id = JobId(
                        job_proto
                            .id
                            .parse::<Uuid>()
                            .wrap_err("server returned invalid job ID")?,
                    );

                    yield Job {
                        client: self.clone(),
                        id: job_id,
                        raw: Some(job_proto),
                        phantom: PhantomData,
                    };
                    total_count += 1;
                }

                next_page_token = response.next_page_token;

                if next_page_token.is_none() {
                    break;
                }
            }
        })
    }

    /// Return the first job that matches the given filters, if any.
    ///
    /// This is just a convenience function that calls `list_jobs` with a limit of 1.
    pub async fn first_job(
        &self,
        filters: JobFilters,
        order: JobOrderBy,
    ) -> crate::Result<Option<Job<AnyJobType>>> {
        pin!(self.list_jobs(filters, order, Some(1)))
            .try_next()
            .await
    }

    /// Return whether any jobs exist based on the given filters.
    pub async fn job_exists(&self, filters: JobFilters) -> crate::Result<bool> {
        // As of writing this the API doesn't have a separate endpoint
        // for this, so we just count the jobs.
        let count = self.count_jobs(filters).await?;
        Ok(count > 0)
    }

    /// Count the number of jobs based on the given filters.
    pub async fn count_jobs(&self, filters: JobFilters) -> crate::Result<u64> {
        Ok(self
            .inner
            .count_jobs(Request::new(proto::admin::v1::CountJobsRequest {
                filters: Some(filters.into()),
            }))
            .await?
            .into_inner()
            .count)
    }

    /// Cancel all jobs that match the given filters.
    ///
    /// Returns the cancelled jobs.
    pub async fn cancel_jobs(&self, filters: JobFilters) -> crate::Result<Vec<Job<AnyJobType>>> {
        Ok(self
            .inner
            .cancel_jobs(Request::new(proto::admin::v1::CancelJobsRequest {
                filters: Some(filters.into()),
            }))
            .await?
            .into_inner()
            .cancelled_job_ids
            .into_iter()
            .map(|id| {
                Ok(Job {
                    client: self.clone(),
                    id: JobId(id.parse()?),
                    raw: None,
                    phantom: PhantomData,
                })
            })
            .collect::<Result<Vec<_>, eyre::Report>>()?)
    }
}

/// Filters for querying jobs.
#[derive(Debug, Default, Clone)]
#[must_use]
pub struct JobFilters {
    /// Filter by job IDs.
    pub job_ids: Option<Vec<JobId>>,
    /// Filter by job type IDs.
    pub job_type_ids: Option<Vec<JobTypeId>>,
    /// Filter by executor IDs.
    pub executor_ids: Option<Vec<ExecutorId>>,
    /// Filter by execution IDs.
    pub execution_ids: Option<Vec<ExecutionId>>,
    /// Execution status.
    pub execution_statuses: Option<Vec<ExecutionStatus>>,
    /// Filter by target execution time range.
    pub target_execution_time: Option<TimeRange>,
    /// Filter by creation time range.
    pub created_at: Option<TimeRange>,
    /// Filter by labels.
    pub labels: Option<Vec<LabelFilter>>,
    /// Filter by schedule IDs.
    pub schedule_ids: Option<Vec<ScheduleId>>,
}

impl JobFilters {
    /// Include all jobs.
    pub fn all() -> Self {
        Self::default()
    }

    /// Filter for a job type.
    pub fn job_type<J: JobType>(mut self) -> Self {
        self.job_type_ids = Some(vec![J::job_type_id()]);
        self
    }

    /// Filter for completed (succeeded, failed, or cancelled) jobs only.
    pub fn completed_only(mut self) -> Self {
        self.execution_statuses = Some(vec![
            ExecutionStatus::Succeeded,
            ExecutionStatus::Failed,
            ExecutionStatus::Cancelled,
        ]);

        self
    }

    /// Filter for pending or in-progress jobs only.
    pub fn active_only(mut self) -> Self {
        self.execution_statuses = Some(vec![ExecutionStatus::Pending, ExecutionStatus::InProgress]);

        self
    }

    /// Filter by existence of a label.
    pub fn has_label<K: Into<String>>(mut self, key: K) -> Self {
        self.labels.get_or_insert_with(Vec::new).push(LabelFilter {
            key: key.into(),
            value: None,
        });

        self
    }

    /// Filter by a label key and value.
    pub fn has_label_value<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.labels.get_or_insert_with(Vec::new).push(LabelFilter {
            key: key.into(),
            value: Some(value.into()),
        });

        self
    }
}

/// The ordering options for listing jobs.
#[derive(Debug, Default, Clone, Copy)]
pub enum JobOrderBy {
    /// Order by target execution time ascending.
    #[default]
    TargetExecutionTimeAsc,
    /// Order by target execution time descending.
    TargetExecutionTimeDesc,
    /// Order by creation time ascending.
    CreatedAtAsc,
    /// Order by creation time descending.
    CreatedAtDesc,
}

/// A job in the ora scheduler.
pub struct Job<J> {
    client: AdminClient,
    id: JobId,
    raw: Option<proto::admin::v1::Job>,
    phantom: PhantomData<J>,
}

impl<J> Job<J> {
    /// Return the ID of the job.
    #[must_use]
    pub fn id(&self) -> JobId {
        self.id
    }

    /// Fetch and return the raw job without any processing.
    pub async fn raw(&mut self) -> crate::Result<proto::admin::v1::Job> {
        let job = self.fetch_raw().await?;

        Ok(job
            .or_else(|| self.raw.clone())
            .ok_or_eyre("failed to retrieve job")?)
    }

    /// Return the cached raw job, if any.
    ///
    /// This will not fetch any data from the server.
    #[must_use]
    pub fn raw_cached(&self) -> Option<&proto::admin::v1::Job> {
        self.raw.as_ref()
    }

    /// Turn this job into its raw representation,
    /// if it was cached.
    #[must_use]
    pub fn into_raw(self) -> Option<proto::admin::v1::Job> {
        self.raw
    }

    /// Cancel the job.
    pub async fn cancel(&mut self) -> crate::Result<()> {
        self.client
            .inner
            .cancel_jobs(Request::new(proto::admin::v1::CancelJobsRequest {
                filters: Some(proto::admin::v1::JobFilters {
                    job_ids: vec![self.id.to_string()],
                    ..Default::default()
                }),
            }))
            .await?;

        Ok(())
    }

    /// Fetch and return the executions of the job.
    pub async fn executions(&mut self) -> crate::Result<Vec<Execution<J>>> {
        let job = self.fetch_raw().await?;

        let job = job
            .as_ref()
            .or(self.raw.as_ref())
            .ok_or_eyre("failed to fetch job")?;

        Ok(job
            .executions
            .iter()
            .map(|e| {
                Ok(Execution {
                    id: ExecutionId(e.id.parse()?),
                    status: match e.status() {
                        proto::admin::v1::ExecutionStatus::Unspecified => {
                            bail!("unknown execution status")
                        }
                        proto::admin::v1::ExecutionStatus::Pending => ExecutionStatus::Pending,
                        proto::admin::v1::ExecutionStatus::InProgress => {
                            ExecutionStatus::InProgress
                        }
                        proto::admin::v1::ExecutionStatus::Succeeded => ExecutionStatus::Succeeded,
                        proto::admin::v1::ExecutionStatus::Failed => ExecutionStatus::Failed,
                        proto::admin::v1::ExecutionStatus::Cancelled => ExecutionStatus::Cancelled,
                    },
                    executor_id: e
                        .executor_id
                        .as_ref()
                        .map(|id| Result::<_, eyre::Report>::Ok(ExecutorId(id.parse()?)))
                        .transpose()?,

                    created_at: e
                        .created_at
                        .map(TryFrom::try_from)
                        .transpose()?
                        .ok_or_eyre("missing created_at for execution")?,
                    started_at: e.started_at.map(TryFrom::try_from).transpose()?,
                    succeeded_at: e.succeeded_at.map(TryFrom::try_from).transpose()?,
                    failed_at: e.failed_at.map(TryFrom::try_from).transpose()?,
                    cancelled_at: e.cancelled_at.map(TryFrom::try_from).transpose()?,
                    output_json: e.output_json.clone(),
                    failure_reason: e.failure_reason.clone(),
                    _job_type: PhantomData,
                })
            })
            .collect::<Result<Vec<_>, eyre::Report>>()?)
    }

    /// Whether the job has terminated.
    pub async fn is_terminated(&mut self) -> crate::Result<bool> {
        let exc = self.executions().await?;

        let Some(last_execution) = exc.last() else {
            return Ok(false);
        };

        Ok(last_execution.status().is_terminal())
    }

    /// Wait for the job to terminate.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn terminated(&mut self) -> crate::Result<()> {
        loop {
            let executions = self.executions().await?;

            let Some(last_execution) = executions.last() else {
                return Err(eyre::eyre!("no executions found for job").into());
            };

            if last_execution.status().is_terminal() {
                return Ok(());
            }

            tokio::time::sleep(self.client.poll_interval).await;
        }
    }

    /// Wait for a change in any of the executions of the job.
    ///
    /// This will also return if no more execution changes are expected,
    /// i.e. if the last execution has reached a terminal state.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn executions_changed(&mut self) -> crate::Result<()> {
        let mut last_execution = self.executions().await?.last().cloned();

        loop {
            let new_executions = self.executions().await?;

            let new_last_execution = new_executions.last();

            if new_last_execution != last_execution.as_ref() {
                return Ok(());
            }

            last_execution = new_last_execution.cloned();

            tokio::time::sleep(self.client.poll_interval).await;
        }
    }

    /// Fetch the output JSON of the job.
    ///
    /// Returns `None` if the job has not finished
    /// or failed.
    pub async fn output_json(&mut self) -> crate::Result<Option<String>> {
        let Some(last_execution) = self.executions().await?.pop() else {
            return Err(eyre::eyre!("no executions found for job").into());
        };

        match last_execution.status() {
            ExecutionStatus::Succeeded => Ok(last_execution.output_json().map(String::from)),
            _ => Ok(None),
        }
    }

    /// Fetch the failure reason of the job.
    ///
    /// Returns `None` if the job has not failed.
    /// Note that cancellation is not considered a failure.
    pub async fn failure_reason(&mut self) -> crate::Result<Option<String>> {
        let Some(last_execution) = self.executions().await?.pop() else {
            return Err(eyre::eyre!("no executions found for job").into());
        };

        match last_execution.status() {
            ExecutionStatus::Failed => Ok(last_execution.failure_reason().map(String::from)),
            _ => Ok(None),
        }
    }

    /// Return the schedule of the job, if any.
    pub async fn schedule(&mut self) -> crate::Result<Option<Schedule<J>>> {
        let job = self.fetch_raw().await?;

        let job = job
            .as_ref()
            .or(self.raw.as_ref())
            .ok_or_eyre("failed to fetch job")?;

        if let Some(schedule_id) = &job.schedule_id {
            let schedule_id = ScheduleId(
                schedule_id
                    .parse::<Uuid>()
                    .wrap_err("server returned invalid schedule ID")?,
            );

            Ok(Some(Schedule {
                client: self.client.clone(),
                id: schedule_id,
                raw: None,
                phantom: PhantomData,
            }))
        } else {
            Ok(None)
        }
    }

    /// Always fetch the raw raw data from the server.
    ///
    /// Returns `None` if the data was cached, this is
    /// to avoid cloning.
    async fn fetch_raw(&mut self) -> crate::Result<Option<proto::admin::v1::Job>> {
        // We avoid fetching if we already have a job that terminated.
        if let Some(cached) = &self.raw
            && cached
                .executions
                .last()
                .is_some_and(|e| ExecutionStatus::from(e.status()).is_terminal())
        {
            return Ok(None);
        }

        let job = self
            .client
            .inner
            .list_jobs(Request::new(ListJobsRequest {
                filters: Some(proto::admin::v1::JobFilters {
                    job_ids: vec![self.id.to_string()],
                    ..Default::default()
                }),
                order_by: proto::admin::v1::JobOrderBy::Unspecified as _,
                pagination: Some(PaginationOptions {
                    page_size: 1,
                    next_page_token: None,
                }),
            }))
            .await?
            .into_inner()
            .jobs
            .pop()
            .ok_or_eyre("job not found")?;

        match self.client.caching_strategy {
            crate::admin::CachingStrategy::Cache => {
                self.raw = Some(job);
                Ok(None)
            }
            crate::admin::CachingStrategy::NoCache => Ok(Some(job)),
        }
    }
}

impl Job<AnyJobType> {
    /// Try to cast the job to the given job type,
    /// validating that the job is of the expected type.
    ///
    /// This function will fetch the required data
    /// from the server if necessary.
    pub async fn cast<J>(mut self) -> crate::Result<Option<Job<J>>>
    where
        J: JobType,
    {
        let job = self.fetch_raw().await?;

        let job = job
            .as_ref()
            .or(self.raw.as_ref())
            .ok_or_eyre("failed to fetch job")?;

        let job = job.job.as_ref().ok_or_eyre("job definition is missing")?;

        let job_type_id = &job.job_type_id;

        if job_type_id != J::job_type_id().as_str() {
            return Ok(None);
        }

        Ok(Some(Job {
            client: self.client,
            id: self.id,
            raw: self.raw,
            phantom: PhantomData,
        }))
    }

    /// Cast this job to the given job type.
    ///
    /// Note that no validation is performed to ensure
    /// that the job is actually of the given type.
    #[must_use]
    pub fn cast_unchecked<J>(self) -> Job<J>
    where
        J: JobType,
    {
        Job {
            client: self.client,
            id: self.id,
            raw: self.raw,
            phantom: PhantomData,
        }
    }

    pub(super) fn cast_any<J>(self) -> Job<J> {
        Job {
            client: self.client,
            id: self.id,
            raw: self.raw,
            phantom: PhantomData,
        }
    }
}

impl<J> Job<J>
where
    J: JobType,
{
    /// Fetch and return the job definition of the job.
    pub async fn definition(&mut self) -> crate::Result<JobDefinition<J>> {
        let job = self.fetch_raw().await?;

        let job = job
            .as_ref()
            .or(self.raw.as_ref())
            .ok_or_eyre("failed to fetch job")?;

        let job = job.job.as_ref().ok_or_eyre("job definition is missing")?;

        let input: J = serde_json::from_str(&job.input_payload_json)
            .wrap_err("failed to deserialize job input")?;

        Ok(JobDefinition {
            target_execution_time: job
                .target_execution_time
                .as_ref()
                .copied()
                .map(TryFrom::try_from)
                .transpose()
                .wrap_err("invalid target execution time")?
                .ok_or_eyre("missing target execution time for job")?,
            input,
            labels: job
                .labels
                .iter()
                .map(|label| (label.key.clone(), label.value.clone()))
                .collect(),
            timeout_policy: job.timeout_policy.unwrap_or_default().into(),
            retry_policy: job.retry_policy.unwrap_or_default().into(),
        })
    }

    /// Wait for the job to complete and retrieve
    /// its result.
    ///
    /// If a job was cancelled, it will be treated as an error.
    pub async fn wait_result(&mut self) -> Result<<J as JobType>::Output, crate::Error> {
        self.terminated().await?;

        let Some(last_execution) = self.executions().await?.pop() else {
            return Err(eyre::eyre!("no executions found for job").into());
        };

        match last_execution.status() {
            ExecutionStatus::Succeeded => Ok(last_execution
                .output()?
                .ok_or_eyre("job succeeded but no output was found")?),
            ExecutionStatus::Failed => Err(crate::Error::JobFailed(
                last_execution
                    .failure_reason()
                    .wrap_err("job failed but no reason was provided")?
                    .into(),
            )),
            ExecutionStatus::Cancelled => Err(crate::Error::JobCancelled),
            _ => {
                Err(eyre::eyre!("job is in unexpected state: {:?}", last_execution.status()).into())
            }
        }
    }

    async fn into_result(mut self) -> crate::Result<J::Output> {
        self.wait_result().await
    }
}

impl<J> IntoFuture for Job<J>
where
    J: JobType,
{
    type Output = crate::Result<J::Output>;

    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        self.into_result().boxed()
    }
}

/// Details of an execution of a job.
#[derive(Debug)]
pub struct Execution<J> {
    id: ExecutionId,
    executor_id: Option<ExecutorId>,
    status: ExecutionStatus,
    created_at: SystemTime,
    started_at: Option<SystemTime>,
    succeeded_at: Option<SystemTime>,
    failed_at: Option<SystemTime>,
    cancelled_at: Option<SystemTime>,
    output_json: Option<String>,
    failure_reason: Option<String>,
    _job_type: PhantomData<J>,
}

impl<J> PartialEq for Execution<J> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.executor_id == other.executor_id
            && self.status == other.status
            && self.created_at == other.created_at
            && self.started_at == other.started_at
            && self.succeeded_at == other.succeeded_at
            && self.failed_at == other.failed_at
            && self.cancelled_at == other.cancelled_at
            && self.output_json == other.output_json
            && self.failure_reason == other.failure_reason
            && self._job_type == other._job_type
    }
}

impl<J> Eq for Execution<J> {}

impl<J> Clone for Execution<J> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            executor_id: self.executor_id,
            status: self.status,
            created_at: self.created_at,
            started_at: self.started_at,
            succeeded_at: self.succeeded_at,
            failed_at: self.failed_at,
            cancelled_at: self.cancelled_at,
            output_json: self.output_json.clone(),
            failure_reason: self.failure_reason.clone(),
            _job_type: PhantomData,
        }
    }
}

impl<J> Execution<J> {
    /// Return the ID of the execution.
    #[inline]
    #[must_use]
    pub const fn id(&self) -> ExecutionId {
        self.id
    }

    /// Return the executor ID that executed the job, if any.
    #[inline]
    #[must_use]
    pub const fn executor_id(&self) -> Option<ExecutorId> {
        self.executor_id
    }

    /// Return the status of the execution.
    #[inline]
    #[must_use]
    pub const fn status(&self) -> ExecutionStatus {
        self.status
    }

    /// Return the timestamp when the execution was created.
    #[inline]
    #[must_use]
    pub const fn created_at(&self) -> SystemTime {
        self.created_at
    }

    /// Return the timestamp when the execution started.
    #[inline]
    #[must_use]
    pub const fn started_at(&self) -> Option<SystemTime> {
        self.started_at
    }

    /// Return the timestamp when the execution successfully completed.
    #[inline]
    #[must_use]
    pub const fn succeeded_at(&self) -> Option<SystemTime> {
        self.succeeded_at
    }

    /// Return the timestamp when the execution failed.
    #[inline]
    #[must_use]
    pub const fn failed_at(&self) -> Option<SystemTime> {
        self.failed_at
    }

    /// Return the timestamp when the execution was cancelled.
    #[inline]
    #[must_use]
    pub const fn cancelled_at(&self) -> Option<SystemTime> {
        self.cancelled_at
    }

    /// Get the end time of the execution, if any.
    ///
    /// This is the time when the execution either succeeded, failed, or was cancelled.
    #[inline]
    #[must_use]
    pub fn ended_at(&self) -> Option<SystemTime> {
        self.succeeded_at.or(self.failed_at).or(self.cancelled_at)
    }

    /// Parse the output of the execution.
    pub fn output(&self) -> crate::Result<Option<J::Output>>
    where
        J: JobType,
    {
        if let Some(ref output_json) = self.output_json {
            let output = serde_json::from_str::<J::Output>(output_json)?;
            Ok(Some(output))
        } else {
            Ok(None)
        }
    }

    /// Return the raw JSON output of the execution.
    #[inline]
    #[must_use]
    pub fn output_json(&self) -> Option<&str> {
        self.output_json.as_deref()
    }

    /// Return the failure reason if the execution failed or was cancelled.
    #[inline]
    #[must_use]
    pub fn failure_reason(&self) -> Option<&str> {
        self.failure_reason.as_deref()
    }
}
