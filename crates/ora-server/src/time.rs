use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytemuck::NoUninit;

/// UTC timestamp in nanoseconds since the Unix epoch.
#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, NoUninit)]
#[repr(transparent)]
#[must_use]
pub struct UnixNanos(pub u64);

impl std::fmt::Debug for UnixNanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UnixNanos")
            .field(&SystemTime::from(*self))
            .finish()
    }
}

impl UnixNanos {
    /// Get the current timestamp from [`SystemTime`].
    pub fn now() -> Self {
        UnixNanos::from(SystemTime::now())
    }

    /// Get the elapsed time since the given timestamp.
    #[must_use]
    pub fn elapsed(self, since: UnixNanos) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(since.0))
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

impl From<SystemTime> for UnixNanos {
    fn from(value: SystemTime) -> Self {
        UnixNanos(
            value
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos()
                .try_into()
                .unwrap_or(0),
        )
    }
}

impl From<UnixNanos> for SystemTime {
    fn from(value: UnixNanos) -> Self {
        UNIX_EPOCH + Duration::from_nanos(value.0)
    }
}
