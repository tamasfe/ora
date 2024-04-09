//! Ora scheduling framework.
#![warn(clippy::pedantic, missing_docs)]

pub use async_trait::async_trait;
pub use eyre;
#[cfg(feature = "api")]
pub use ora_api::{client::Client, *};
pub use ora_common::{
    schedule::*,
    task::{TaskDataFormat, TaskDefinition, TaskMetadata, TaskStatus, WorkerSelector},
    timeout::TimeoutPolicy,
};
#[cfg(feature = "macros")]
pub use ora_macros::Task;
#[cfg(feature = "scheduler")]
pub use ora_scheduler::scheduler::{Error as SchedulerError, Scheduler};
#[cfg(feature = "store-memory")]
pub use ora_store_memory::{MemoryStore, MemoryStoreOptions};
#[cfg(feature = "store-sqlx-postgres")]
pub use ora_store_sqlx::{DbStore, DbStoreOptions};
#[cfg(feature = "test")]
pub use ora_test as test;
#[cfg(feature = "worker")]
pub use ora_worker::{
    worker::{Error as WorkerError, Worker, WorkerOptions},
    TaskContext,
};

#[cfg(feature = "macros")]
#[doc(hidden)]
pub mod __private {
    #[doc(hidden)]
    pub use schemars;
    #[doc(hidden)]
    pub use serde;
    #[doc(hidden)]
    pub use serde_json;
    #[doc(hidden)]
    pub use time;
}
