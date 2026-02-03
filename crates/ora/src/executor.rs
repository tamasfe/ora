//! Executor definitions and related types.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "executor")]
mod implementation;

#[cfg(feature = "executor")]
pub use implementation::*;

/// An executor ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ExecutorId(pub Uuid);

impl From<Uuid> for ExecutorId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ExecutorId> for Uuid {
    fn from(value: ExecutorId) -> Self {
        value.0
    }
}

impl std::fmt::Display for ExecutorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
