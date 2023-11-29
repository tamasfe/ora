//! Defaults for [`Task`](super::Task) and other traits.

use std::any::type_name;

use ora_common::{
    task::{TaskDataFormat, TaskMetadata, WorkerSelector},
    timeout::TimeoutPolicy,
};

/// The worker selector of the given type,
/// defaults to the type's name without the module path
/// and any generics.
pub fn task_worker_selector<T>() -> WorkerSelector {
    let name = type_name::<T>();
    let name = name.split_once('<').map_or(name, |n| n.0);
    let name = name.split("::").last().unwrap_or(name);

    WorkerSelector {
        kind: std::borrow::Cow::Borrowed(name),
    }
}

/// Input and output data format to be used for serialization.
///
/// Defaults to [`TaskDataFormat::Json`].
#[must_use]
pub fn task_data_format<T>() -> TaskDataFormat {
    TaskDataFormat::Json
}

/// Return the default timeout policy.
///
/// Defaults to no timeout.
#[must_use]
pub fn task_timeout<T>() -> TimeoutPolicy {
    TimeoutPolicy::Never
}

/// Return additional metadata about the task.
#[must_use]
pub fn task_metadata<T>() -> TaskMetadata {
    TaskMetadata::default()
}
