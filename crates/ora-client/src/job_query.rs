//! Query filters for jobs.

use ora_proto::server::v1::{self, JobExecutionStatus, JobQueryOrder};
use uuid::Uuid;

use crate::{job_definition::JobStatus, IndexSet, JobType};

/// A filter for querying jobs.
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct JobFilter {
    /// The job IDs to filter by.
    ///
    /// If the list is empty, all jobs are included.
    pub job_ids: IndexSet<Uuid>,
    /// The job type IDs to filter by.
    ///
    /// If the list is empty, all job types are included.
    pub job_type_ids: IndexSet<String>,
    /// The execution IDs to filter by.
    ///
    /// If the list is empty, all executors are included.
    pub execution_ids: IndexSet<Uuid>,
    /// The schedule IDs to filter by.
    pub schedule_ids: IndexSet<Uuid>,
    /// A list of execution statuses to filter by.
    /// If the list is empty, all statuses are included.
    ///
    /// If a job has multiple executions, the last
    /// execution status is used.
    ///
    /// If a job has no executions, its status is
    /// considered to be pending.
    pub status: IndexSet<JobStatus>,
    /// A list of labels to filter by.
    ///
    /// If multiple filters are specified, all of them
    /// must match.
    pub labels: Vec<JobLabelFilter>,
    /// Only include active or inactive jobs.
    ///
    /// If not provided, all jobs are included.
    pub active: Option<bool>,
}

impl JobFilter {
    /// Create a new job filter that includes all jobs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by a specific job ID.
    pub fn with_job_id(mut self, job_id: Uuid) -> Self {
        self.job_ids.insert(job_id);
        self
    }

    /// Filter by specific job IDs.
    pub fn with_job_ids(mut self, job_ids: impl IntoIterator<Item = Uuid>) -> Self {
        self.job_ids.extend(job_ids);
        self
    }

    /// Filter by a specific job type ID.
    pub fn with_job_type_id(mut self, job_type_id: impl Into<String>) -> Self {
        self.job_type_ids.insert(job_type_id.into());
        self
    }

    /// Filter by specific job type IDs.
    pub fn with_job_type_ids(mut self, job_type_ids: impl IntoIterator<Item = String>) -> Self {
        self.job_type_ids.extend(job_type_ids);
        self
    }

    /// Filter by a specific execution ID.
    pub fn with_execution_id(mut self, execution_id: Uuid) -> Self {
        self.execution_ids.insert(execution_id);
        self
    }

    /// Filter by specific execution IDs.
    pub fn with_execution_ids(mut self, execution_ids: impl IntoIterator<Item = Uuid>) -> Self {
        self.execution_ids.extend(execution_ids);
        self
    }

    /// Filter by a specific schedule ID.
    pub fn with_schedule_id(mut self, schedule_id: Uuid) -> Self {
        self.schedule_ids.insert(schedule_id);
        self
    }

    /// Filter by specific schedule IDs.
    pub fn with_schedule_ids(mut self, schedule_ids: impl IntoIterator<Item = Uuid>) -> Self {
        self.schedule_ids.extend(schedule_ids);
        self
    }

    /// Filter by a specific status.
    pub fn include_status(mut self, status: JobStatus) -> Self {
        self.status.insert(status);
        self
    }

    /// Include pending jobs.
    pub fn include_pending(mut self) -> Self {
        self.status.insert(JobStatus::Pending);
        self
    }

    /// Include ready jobs.
    pub fn include_ready(mut self) -> Self {
        self.status.insert(JobStatus::Ready);
        self
    }

    /// Include assigned jobs.
    pub fn include_assigned(mut self) -> Self {
        self.status.insert(JobStatus::Assigned);
        self
    }

    /// Include running jobs.
    pub fn include_running(mut self) -> Self {
        self.status.insert(JobStatus::Running);
        self
    }

    /// Include successful jobs.
    pub fn include_succeeded(mut self) -> Self {
        self.status.insert(JobStatus::Succeeded);
        self
    }

    /// Include failed jobs.
    pub fn include_failed(mut self) -> Self {
        self.status.insert(JobStatus::Failed);
        self
    }

    /// Filter by active status.
    pub fn active_only(mut self) -> Self {
        self.active = Some(true);
        self
    }

    /// Filter by inactive status.
    pub fn inactive_only(mut self) -> Self {
        self.active = Some(false);
        self
    }

    /// Filter by a label.
    pub fn with_label_value(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push(JobLabelFilter {
            key: key.into(),
            value: JobLabelFilterValue::Equals(value.into()),
        });
        self
    }

    /// Filter by a label that must exist.
    pub fn with_label(mut self, key: impl Into<String>) -> Self {
        self.labels.push(JobLabelFilter {
            key: key.into(),
            value: JobLabelFilterValue::Exists,
        });
        self
    }

    /// Include a given job type.
    pub fn include_job_type<J>(self) -> Self
    where
        J: JobType,
    {
        self.with_job_type_id(J::id())
    }
}

impl From<JobFilter> for v1::JobQueryFilter {
    fn from(filter: JobFilter) -> Self {
        Self {
            job_ids: filter
                .job_ids
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            job_type_ids: filter
                .job_type_ids
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            execution_ids: filter
                .execution_ids
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            schedule_ids: filter
                .schedule_ids
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            status: filter
                .status
                .into_iter()
                .map(|s| JobExecutionStatus::from(s).into())
                .collect(),
            labels: filter.labels.into_iter().map(Into::into).collect(),
            active: filter.active,
        }
    }
}

/// A filter for querying jobs by label.
#[derive(Debug, Clone)]
pub struct JobLabelFilter {
    /// The key of the label.
    pub key: String,
    /// The condition for the label value.
    pub value: JobLabelFilterValue,
}

/// The condition for a label filter.
#[derive(Debug, Clone)]
pub enum JobLabelFilterValue {
    /// Any label value must exist with the key.
    Exists,
    /// The label value must be equal to the provided value.
    Equals(String),
}

impl From<JobLabelFilter> for v1::JobLabelFilter {
    fn from(filter: JobLabelFilter) -> Self {
        Self {
            key: filter.key,
            value: match filter.value {
                JobLabelFilterValue::Exists => Some(v1::job_label_filter::Value::Exists(
                    v1::LabelFilterExistCondition::Exists.into(),
                )),
                JobLabelFilterValue::Equals(value) => {
                    Some(v1::job_label_filter::Value::Equals(value))
                }
            },
        }
    }
}

/// The order of jobs returned in a query.
#[derive(Debug, Default, Clone, Copy)]
pub enum JobOrder {
    /// Order by the time the job was created in ascending order.
    CreatedAtAsc,
    /// Order by the time the job was created in descending order.
    #[default]
    CreatedAtDesc,
    /// Order by the target execution time in ascending order.
    TargetExecutionTimeAsc,
    /// Order by the target execution time in descending order.
    TargetExecutionTimeDesc,
}

impl From<JobOrder> for JobQueryOrder {
    fn from(value: JobOrder) -> Self {
        match value {
            JobOrder::CreatedAtAsc => Self::CreatedAtAsc,
            JobOrder::CreatedAtDesc => Self::CreatedAtDesc,
            JobOrder::TargetExecutionTimeAsc => Self::TargetExecutionTimeAsc,
            JobOrder::TargetExecutionTimeDesc => Self::TargetExecutionTimeDesc,
        }
    }
}
