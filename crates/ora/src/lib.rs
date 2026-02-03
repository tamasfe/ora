//! Client library for the ora scheduler.

pub mod admin;
pub mod common;
pub mod errors;
pub mod execution;
pub mod executor;
pub mod job;
pub mod job_type;
pub mod proto;
pub mod schedule;

pub use admin::{
    AdminClient,
    jobs::{Execution, Job, JobFilters, JobOrderBy},
    schedules::{Schedule, ScheduleFilters, ScheduleOrderBy, ScheduleStatus},
};
pub use job::{IntoJob, JobDefinition};
pub use job_type::{JobType, JobTypeId};
pub use ora_macros::JobType;
pub use schedule::ScheduleDefinition;

#[cfg(feature = "server")]
pub mod server;

pub use errors::Error;
pub use errors::Result;
