//! Client library for the Ora job scheduler.

pub mod admin;
pub mod executor;
pub mod snapshot;

pub mod job_definition;
pub mod job_handle;
pub mod job_query;
pub mod job_type;

pub mod schedule_definition;
pub mod schedule_handle;
pub mod schedule_query;

pub use admin::AdminClient;
pub use executor::{ExecutionContext, Executor, ExecutorOptions};
pub use job_definition::JobDefinition;
pub use job_type::{JobType, TypedJobDefinition};

#[allow(missing_docs)]
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, ahash::RandomState>;
#[allow(missing_docs)]
pub type IndexSet<T> = indexmap::IndexSet<T, ahash::RandomState>;

mod proto_convert;
