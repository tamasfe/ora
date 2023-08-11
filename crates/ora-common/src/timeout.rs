//! Task timeout types.

use serde::{Deserialize, Serialize};
use time::Duration;

/// A timeout policy for a given task.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub enum TimeoutPolicy {
    /// Never time out.
    #[default]
    Never,
    /// The timeout duration applies from the
    /// task target.
    FromTarget {
        /// The timeout duration.
        timeout: Duration,
    },
}

impl TimeoutPolicy {
    /// Set a timeout duration that applies after
    /// the task target.
    #[must_use]
    pub fn from_target(timeout: Duration) -> Self {
        Self::FromTarget { timeout }
    }
}

impl From<Duration> for TimeoutPolicy {
    fn from(timeout: Duration) -> Self {
        Self::FromTarget { timeout }
    }
}
