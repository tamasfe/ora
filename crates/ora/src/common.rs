//! Common types and utilities.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// A label associated with an entity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Label {
    /// The key of the label.
    pub key: String,
    /// The value of the label.
    pub value: String,
}

/// A filter for labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelFilter {
    /// The key of the label to filter by.
    pub key: String,
    /// An optional value of the label to filter by,
    /// if not specified, entities with
    /// the given key and any value will match.
    pub value: Option<String>,
}

/// A time range that can be open-ended on either side.
///
/// The end of the range is exclusive.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    /// The start of the time range.
    ///
    /// If `None`, the range is open-ended at the start.
    pub start: Option<SystemTime>,
    /// The end of the time range.
    ///
    /// If `None`, the range is open-ended at the end.
    pub end: Option<SystemTime>,
}

/// A type that is returned from `if_not_exists` requests,
/// a resource was either added or existing ones are returned.
pub enum AddedOrExisting<T> {
    /// The resource was added.
    Added(T),
    /// The resource already existed.
    Existing(T),
}

impl<T> AddedOrExisting<T> {
    /// Unwrap the inner value.
    #[must_use]
    pub fn into_inner(self) -> T {
        match self {
            AddedOrExisting::Added(job) | AddedOrExisting::Existing(job) => job,
        }
    }

    /// Return whether the job was added or already existed.
    #[must_use]
    pub fn is_added(&self) -> bool {
        matches!(self, AddedOrExisting::Added(_))
    }

    /// Return whether the job already existed.
    #[must_use]
    pub fn is_existing(&self) -> bool {
        matches!(self, AddedOrExisting::Existing(_))
    }
}
