//! Executions and related types.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An execution ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ExecutionId(pub Uuid);

impl std::fmt::Display for ExecutionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ExecutionId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ExecutionId> for Uuid {
    fn from(value: ExecutionId) -> Self {
        value.0
    }
}

/// The status of an execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// The execution is pending.
    Pending,
    /// The execution is in progress.
    InProgress,
    /// The execution has completed successfully.
    Succeeded,
    /// The execution has failed.
    Failed,
    /// The execution was cancelled.
    Cancelled,
}

impl ExecutionStatus {
    /// Return whether the execution is in a terminal state.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            ExecutionStatus::Succeeded | ExecutionStatus::Failed | ExecutionStatus::Cancelled
        )
    }
}
