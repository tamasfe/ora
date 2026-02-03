//! Schedule management.

use std::{cmp, marker::PhantomData};

use eyre::{Context, OptionExt};
use futures::{Stream, TryStreamExt};
use tonic::Request;
use uuid::Uuid;

use crate::{
    admin::{
        AdminClient,
        jobs::{Job, JobFilters, JobOrderBy},
    },
    common::{LabelFilter, TimeRange},
    job_type::{AnyJobType, JobType, JobTypeId},
    proto::{
        self,
        admin::v1::{AddSchedulesRequest, ListSchedulesRequest, PaginationOptions},
    },
    schedule::{ScheduleDefinition, ScheduleId},
};

impl AdminClient {
    /// Add a new schedule.
    pub async fn add_schedule<J>(
        &self,
        schedule: ScheduleDefinition<J>,
    ) -> crate::Result<Schedule<J>>
    where
        J: JobType,
    {
        let id = self
            .inner
            .add_schedules(Request::new(AddSchedulesRequest {
                schedules: vec![schedule.try_into()?],
                if_not_exists: None,
                inherit_labels: Some(true),
            }))
            .await?
            .into_inner()
            .schedule_ids
            .pop()
            .ok_or_eyre("schedule ID not returned after creating the schedule")?
            .parse::<Uuid>()
            .wrap_err("invalid schedule ID")
            .map(ScheduleId)?;

        Ok(Schedule {
            id,
            client: self.clone(),
            raw: None,
            phantom: PhantomData,
        })
    }

    /// Add multiple schedules.
    pub async fn add_schedules<I>(&self, schedules: I) -> crate::Result<Vec<Schedule<AnyJobType>>>
    where
        I: IntoIterator<Item: TryInto<proto::schedules::v1::Schedule, Error: Into<crate::Error>>>,
    {
        let schedules = schedules
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)?;

        self.inner
            .add_schedules(Request::new(AddSchedulesRequest {
                schedules,
                if_not_exists: None,
                inherit_labels: Some(true),
            }))
            .await?
            .into_inner()
            .schedule_ids
            .into_iter()
            .map(|id| {
                Result::<_, crate::Error>::Ok(Schedule {
                    client: self.clone(),
                    id: ScheduleId(
                        id.parse::<Uuid>()
                            .wrap_err("server returned invalid schedule ID")?,
                    ),
                    raw: None,
                    phantom: PhantomData,
                })
            })
            .collect::<Result<Vec<_>, _>>()
    }

    /// Add a schedule if no schedules exist with the given filter.
    pub async fn add_schedule_if_not_exists<J>(
        &self,
        schedule: ScheduleDefinition<J>,
        filters: ScheduleFilters,
    ) -> crate::Result<Option<Schedule<J>>>
    where
        J: JobType,
    {
        let id = self
            .inner
            .add_schedules(Request::new(AddSchedulesRequest {
                schedules: vec![schedule.try_into()?],
                if_not_exists: Some(filters.into()),
                inherit_labels: Some(true),
            }))
            .await?
            .into_inner()
            .schedule_ids
            .pop();

        let Some(id) = id else {
            return Ok(None);
        };

        let id = id
            .parse::<Uuid>()
            .wrap_err("invalid schedule ID")
            .map(ScheduleId)?;

        Ok(Some(Schedule {
            id,
            client: self.clone(),
            raw: None,
            phantom: PhantomData,
        }))
    }

    /// Add multiple schedules if no schedules exist with the given filter.
    pub async fn add_schedules_if_not_exists<I>(
        &self,
        schedules: I,
        filters: ScheduleFilters,
    ) -> crate::Result<Vec<Schedule<AnyJobType>>>
    where
        I: IntoIterator<Item: TryInto<proto::schedules::v1::Schedule, Error = crate::Error>>,
    {
        let schedules = schedules
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        self.inner
            .add_schedules(Request::new(AddSchedulesRequest {
                schedules,
                if_not_exists: Some(filters.into()),
                inherit_labels: Some(true),
            }))
            .await?
            .into_inner()
            .schedule_ids
            .into_iter()
            .map(|id| {
                Result::<_, crate::Error>::Ok(Schedule {
                    client: self.clone(),
                    id: ScheduleId(
                        id.parse::<Uuid>()
                            .wrap_err("server returned invalid schedule ID")?,
                    ),
                    raw: None,
                    phantom: PhantomData,
                })
            })
            .collect::<Result<Vec<_>, _>>()
    }

    /// List schedules based on the given filters.
    pub fn list_schedules(
        &self,
        filters: ScheduleFilters,
        order: ScheduleOrderBy,
        limit: Option<u32>,
    ) -> impl Stream<Item = crate::Result<Schedule<AnyJobType>>> {
        async_stream::try_stream!({
            let mut total_count = 0;
            let mut next_page_token = None;

            let filters: proto::admin::v1::ScheduleFilters = filters.into();

            loop {
                if let Some(limit) = limit
                    && total_count >= limit
                {
                    break;
                }

                let response = self
                    .inner
                    .list_schedules(Request::new(ListSchedulesRequest {
                        filters: Some(filters.clone()),
                        order_by: match order {
                            ScheduleOrderBy::CreatedAtAsc => {
                                proto::admin::v1::ScheduleOrderBy::CreatedAtAsc as i32
                            }
                            ScheduleOrderBy::CreatedAtDesc => {
                                proto::admin::v1::ScheduleOrderBy::CreatedAtDesc as i32
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

                for schedule_proto in response.schedules {
                    let schedule_id = ScheduleId(
                        schedule_proto
                            .id
                            .parse::<Uuid>()
                            .wrap_err("server returned invalid schedule ID")?,
                    );

                    yield Schedule {
                        client: self.clone(),
                        id: schedule_id,
                        raw: Some(schedule_proto),
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

    /// Count the amount of schedules matching the given filters.
    pub async fn count_schedules(&self, filters: ScheduleFilters) -> crate::Result<u64> {
        let response = self
            .inner
            .count_schedules(Request::new(proto::admin::v1::CountSchedulesRequest {
                filters: Some(filters.into()),
            }))
            .await?
            .into_inner();

        Ok(response.count)
    }

    /// Stop the schedules matching the given filters.
    pub async fn stop_schedules(
        &self,
        filters: ScheduleFilters,
        job_action: StoppedScheduleJobAction,
    ) -> crate::Result<Vec<Schedule<AnyJobType>>> {
        Ok(self
            .inner
            .stop_schedules(Request::new(proto::admin::v1::StopSchedulesRequest {
                filters: Some(filters.into()),
                cancel_active_jobs: matches!(job_action, StoppedScheduleJobAction::Cancel),
            }))
            .await?
            .into_inner()
            .cancelled_schedule_ids
            .into_iter()
            .map(|id| {
                Ok(Schedule {
                    client: self.clone(),
                    id: ScheduleId(id.parse()?),
                    raw: None,
                    phantom: PhantomData,
                })
            })
            .collect::<Result<Vec<_>, eyre::Report>>()?)
    }

    /// Return whether any schedules exist matching the given filters.
    pub async fn schedule_exists(&self, filters: ScheduleFilters) -> crate::Result<bool> {
        // As of writing this the API doesn't have a separate endpoint
        // for this, so we just count the schedules.
        let count = self.count_schedules(filters).await?;
        Ok(count > 0)
    }
}

/// A schedule in the ora scheduler.
pub struct Schedule<J> {
    pub(super) client: AdminClient,
    /// The unique ID of the schedule.
    pub(super) id: ScheduleId,
    /// The raw schedule details.
    pub(super) raw: Option<proto::admin::v1::Schedule>,
    pub(super) phantom: PhantomData<J>,
}

impl<J> Schedule<J> {
    /// Return the ID of this schedule.
    #[must_use]
    pub fn id(&self) -> ScheduleId {
        self.id
    }

    /// Stop this schedule.
    pub async fn stop(&self, job_action: StoppedScheduleJobAction) -> crate::Result<()> {
        self.client
            .stop_schedules(
                ScheduleFilters {
                    schedule_ids: Some(vec![self.id]),
                    ..Default::default()
                },
                job_action,
            )
            .await?;

        Ok(())
    }

    /// Fetch jobs of this schedule with the given additional filters.
    pub fn list_jobs(
        &self,
        mut filters: JobFilters,
        order: JobOrderBy,
        limit: Option<u32>,
    ) -> impl Stream<Item = crate::Result<Job<J>>> {
        filters.schedule_ids = Some(vec![self.id]);

        self.client
            .list_jobs(filters, order, limit)
            .map_ok(Job::cast_any)
    }

    /// Return the status of the schedule.
    pub async fn status(&mut self) -> crate::Result<ScheduleStatus> {
        let raw = self.fetch_raw().await?;

        let raw = raw
            .as_ref()
            .or(self.raw.as_ref())
            .ok_or_eyre("failed to retrieve schedule")?;

        match raw.status() {
            proto::admin::v1::ScheduleStatus::Active
            | proto::admin::v1::ScheduleStatus::Unspecified => Ok(ScheduleStatus::Active),
            proto::admin::v1::ScheduleStatus::Stopped => Ok(ScheduleStatus::Stopped),
        }
    }

    /// Fetch and return the raw data without any processing.
    pub async fn raw(&mut self) -> crate::Result<proto::admin::v1::Schedule> {
        let job = self.fetch_raw().await?;

        Ok(job
            .or_else(|| self.raw.clone())
            .ok_or_eyre("failed to retrieve schedule")?)
    }

    /// Return the cached raw schedule, if any.
    ///
    /// This will not fetch any data from the server.
    #[must_use]
    pub fn raw_cached(&self) -> Option<&proto::admin::v1::Schedule> {
        self.raw.as_ref()
    }

    /// Always fetch the raw raw data from the server.
    ///
    /// Returns `None` if the data was cached, this is
    /// to avoid cloning.
    async fn fetch_raw(&mut self) -> crate::Result<Option<proto::admin::v1::Schedule>> {
        let schedule = self
            .client
            .inner
            .list_schedules(Request::new(ListSchedulesRequest {
                filters: Some(proto::admin::v1::ScheduleFilters {
                    schedule_ids: vec![self.id.to_string()],
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
            .schedules
            .pop()
            .ok_or_eyre("schedule not found")?;

        match self.client.caching_strategy {
            crate::admin::CachingStrategy::Cache => {
                self.raw = Some(schedule);
                Ok(None)
            }
            crate::admin::CachingStrategy::NoCache => Ok(Some(schedule)),
        }
    }
}

impl Schedule<AnyJobType> {
    /// Try to cast the schedule to the given job type,
    /// validating that the schedule is of the expected type.
    ///
    /// This function will fetch the required data
    /// from the server if necessary.
    pub async fn cast<J>(mut self) -> crate::Result<Option<Schedule<J>>>
    where
        J: JobType,
    {
        let schedule = self.fetch_raw().await?;

        let schedule = schedule
            .as_ref()
            .or(self.raw.as_ref())
            .ok_or_eyre("failed to fetch job")?;

        let job = schedule
            .schedule
            .as_ref()
            .ok_or_eyre("job definition is missing")?
            .job_template
            .as_ref()
            .ok_or_eyre("job template is missing")?;

        let job_type_id = &job.job_type_id;

        if job_type_id != J::job_type_id().as_str() {
            return Ok(None);
        }

        Ok(Some(Schedule {
            client: self.client,
            id: self.id,
            raw: self.raw,
            phantom: PhantomData,
        }))
    }

    /// Cast this schedule to the given job type.
    ///
    /// Note that no validation is performed to ensure
    /// that the schedule is actually of the given type.
    #[must_use]
    pub fn cast_unchecked<J>(self) -> Schedule<J>
    where
        J: JobType,
    {
        Schedule {
            client: self.client,
            id: self.id,
            raw: self.raw,
            phantom: PhantomData,
        }
    }
}

/// The status of a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleStatus {
    /// The schedule is active.
    Active,
    /// The schedule was stopped.
    Stopped,
}

impl ScheduleStatus {
    /// Return whether the schedule is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self, ScheduleStatus::Active)
    }
}

/// Filters for listing schedules.
#[derive(Debug, Default)]
#[must_use]
pub struct ScheduleFilters {
    /// Filter by schedule IDs.
    pub schedule_ids: Option<Vec<ScheduleId>>,
    /// Filter by job type IDs.
    pub job_type_ids: Option<Vec<JobTypeId>>,
    /// Filter by status.
    pub statuses: Option<Vec<ScheduleStatus>>,
    /// Filter by creation time.
    /// The range can be open-ended in either direction.
    pub created_at: Option<TimeRange>,
    /// Filter by labels.
    pub labels: Option<Vec<LabelFilter>>,
}

impl ScheduleFilters {
    /// Include all jobs.
    pub fn all() -> Self {
        Self::default()
    }

    /// Filter for a job type.
    pub fn job_type<J: JobType>(mut self) -> Self {
        self.job_type_ids = Some(vec![J::job_type_id()]);
        self
    }

    /// Filter for active schedules only.
    pub fn active_only(mut self) -> Self {
        self.statuses = Some(vec![ScheduleStatus::Active]);
        self
    }

    /// Filter for stopped schedules only.
    pub fn stopped_only(mut self) -> Self {
        self.statuses = Some(vec![ScheduleStatus::Stopped]);
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

/// The ordering options for listing schedules.
#[derive(Debug, Default, Clone, Copy)]
pub enum ScheduleOrderBy {
    /// Order by creation time ascending.
    #[default]
    CreatedAtAsc,
    /// Order by creation time descending.
    CreatedAtDesc,
}

/// What to do with jobs of stopped schedules.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StoppedScheduleJobAction {
    /// Cancel all jobs of the stopped schedules.
    #[default]
    Cancel,
    /// Ignore the jobs of the stopped schedules.
    Ignore,
}
