//! Common types and functions for Ora.

#![warn(clippy::pedantic, missing_docs)]
#![allow(clippy::module_name_repetitions, clippy::default_trait_access)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub mod schedule;
pub mod task;
pub mod timeout;

/// UTC timestamp used by Ora, nanoseconds since the unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
#[must_use]
pub struct UnixNanos(pub u64);

impl UnixNanos {
    /// Get the current timestamp from [`SystemTime`].
    pub fn now() -> Self {
        UnixNanos(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos()
                .try_into()
                .unwrap_or(0),
        )
    }
}

impl From<u64> for UnixNanos {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<UnixNanos> for u64 {
    fn from(value: UnixNanos) -> Self {
        value.0
    }
}

impl From<UnixNanos> for OffsetDateTime {
    #[allow(clippy::cast_lossless)]
    fn from(value: UnixNanos) -> Self {
        OffsetDateTime::from_unix_timestamp_nanos(value.0 as _).unwrap()
    }
}

impl From<OffsetDateTime> for UnixNanos {
    fn from(value: OffsetDateTime) -> Self {
        UnixNanos(value.unix_timestamp_nanos().try_into().unwrap_or(0))
    }
}
