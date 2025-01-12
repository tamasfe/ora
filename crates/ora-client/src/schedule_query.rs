//! Query filters for schedules.

use std::time::SystemTime;

use ora_proto::{
    common::v1::TimeRange,
    server::v1::{self, ScheduleQueryOrder},
};
use uuid::Uuid;

use crate::{IndexSet, JobType};

/// A filter for querying schedules.
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct ScheduleFilter {
    /// The schedule IDs to filter by.
    pub schedule_ids: IndexSet<Uuid>,
    /// The job IDs to filter by.
    ///
    /// If the list is empty, all schedules are included.
    pub job_ids: IndexSet<Uuid>,
    /// The job type IDs to filter by.
    ///
    /// If the list is empty, all job types are included.
    pub job_type_ids: IndexSet<String>,
    /// A list of labels to filter by.
    ///
    /// If multiple filters are specified, all of them
    /// must match.
    pub labels: Vec<ScheduleLabelFilter>,
    /// Only include active or inactive schedules.
    ///
    /// If not provided, all schedules are included.
    pub active: Option<bool>,
    /// Only include schedules created after the provided time.
    ///
    /// The time is inclusive.
    pub created_after: Option<SystemTime>,
    /// Only include schedules created before the provided time.
    ///
    /// The time is exclusive.
    pub created_before: Option<SystemTime>,
}

impl ScheduleFilter {
    /// Create a new job filter that includes all schedules.
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
        self.labels.push(ScheduleLabelFilter {
            key: key.into(),
            value: ScheduleLabelFilterValue::Equals(value.into()),
        });
        self
    }

    /// Filter by a label that must exist.
    pub fn with_label(mut self, key: impl Into<String>) -> Self {
        self.labels.push(ScheduleLabelFilter {
            key: key.into(),
            value: ScheduleLabelFilterValue::Exists,
        });
        self
    }

    /// Filter by schedules created after the provided time.
    ///
    /// The time is inclusive.
    pub fn created_after(mut self, time: SystemTime) -> Self {
        self.created_after = Some(time);
        self
    }

    /// Filter by schedules created before the provided time.
    ///
    /// The time is exclusive.
    pub fn created_before(mut self, time: SystemTime) -> Self {
        self.created_before = Some(time);
        self
    }

    /// Filter by a job type.
    pub fn include_job_type<J: JobType>(self) -> Self {
        self.with_job_type_id(J::id())
    }
}

impl From<ScheduleFilter> for v1::ScheduleQueryFilter {
    fn from(filter: ScheduleFilter) -> Self {
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

            schedule_ids: filter
                .schedule_ids
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            labels: filter.labels.into_iter().map(Into::into).collect(),
            active: filter.active,
            created_at: Some(TimeRange {
                start: filter.created_after.map(Into::into),
                end: filter.created_before.map(Into::into),
            }),
        }
    }
}

/// A label filter for schedules.
#[derive(Debug, Clone)]
pub struct ScheduleLabelFilter {
    /// The key of the label.
    pub key: String,
    /// The condition for the label value.
    pub value: ScheduleLabelFilterValue,
}

/// The condition for a label filter.
#[derive(Debug, Clone)]
pub enum ScheduleLabelFilterValue {
    /// Any label value must exist with the key.
    Exists,
    /// The label value must be equal to the provided value.
    Equals(String),
}

impl From<ScheduleLabelFilter> for v1::ScheduleLabelFilter {
    fn from(filter: ScheduleLabelFilter) -> Self {
        Self {
            key: filter.key,
            value: match filter.value {
                ScheduleLabelFilterValue::Exists => Some(v1::schedule_label_filter::Value::Exists(
                    v1::LabelFilterExistCondition::Exists.into(),
                )),
                ScheduleLabelFilterValue::Equals(value) => {
                    Some(v1::schedule_label_filter::Value::Equals(value))
                }
            },
        }
    }
}

/// The order of schedules returned in a query.
#[derive(Debug, Default, Clone, Copy)]
pub enum ScheduleOrder {
    /// Order by the time the job was created in ascending order.
    CreatedAtAsc,
    /// Order by the time the job was created in descending order.
    #[default]
    CreatedAtDesc,
}

impl From<ScheduleOrder> for ScheduleQueryOrder {
    fn from(value: ScheduleOrder) -> Self {
        match value {
            ScheduleOrder::CreatedAtAsc => Self::CreatedAtAsc,
            ScheduleOrder::CreatedAtDesc => Self::CreatedAtDesc,
        }
    }
}
