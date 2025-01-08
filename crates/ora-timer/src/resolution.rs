//! Resolution adapters for timers.

use core::time::Duration;

/// A timer resolution.
///
/// Finer resolutions allow for more precise timers, coarser
/// resolutions may perform better due to grouping targets that
/// are close together.
pub trait Resolution {
    /// The maximum duration that can be represented as [`u32`]
    /// at this resolution.
    const MAX_DURATION: Duration;

    /// Convert the given duration into timer cycle steps.
    /// The given duration is guaranteed to be smaller than [`Resolution::MAX_DURATION`].
    ///
    /// If `upper_bound` is true, the returned value should be rounded up.
    fn cycle_steps(duration: &Duration, upper_bound: bool) -> u32;

    /// Return total steps required for the given duration.
    /// The returned steps should not be rounded up.
    fn whole_steps(duration: &Duration) -> u64;

    /// Convert the given steps to a duration.
    fn steps_as_duration(steps: u64) -> Duration;
}

/// Microsecond resolution adapter.
#[derive(Debug)]
pub enum MicrosecondResolution {}

impl Resolution for MicrosecondResolution {
    const MAX_DURATION: Duration = Duration::from_micros(u32::MAX as u64);

    #[allow(clippy::cast_possible_truncation)]
    #[inline]
    fn cycle_steps(duration: &Duration, upper_bound: bool) -> u32 {
        let mut steps = (duration.as_secs() * 1_000_000) as u32;
        let micros = duration.subsec_micros();
        steps += micros;
        if upper_bound && duration.subsec_nanos() % 1_000 > 0 {
            steps = steps.saturating_add(1);
        }
        steps
    }

    #[inline]
    fn steps_as_duration(steps: u64) -> Duration {
        Duration::from_micros(steps)
    }

    #[inline]
    #[allow(clippy::cast_possible_truncation)]
    fn whole_steps(duration: &Duration) -> u64 {
        (duration.as_nanos() / 1_000) as u64
    }
}

/// Millisecond resolution adapter.
#[derive(Debug)]
pub enum MillisecondResolution {}

impl Resolution for MillisecondResolution {
    const MAX_DURATION: Duration = Duration::from_millis(u32::MAX as u64);

    #[allow(clippy::cast_possible_truncation)]
    #[inline]
    fn cycle_steps(duration: &Duration, upper_bound: bool) -> u32 {
        let mut steps = (duration.as_secs() * 1000) as u32;
        let ms = duration.subsec_millis();
        steps += ms;

        if upper_bound && duration.subsec_nanos() % 1_000_000 > 0 {
            steps = steps.saturating_add(1);
        }

        steps
    }

    #[inline]
    fn steps_as_duration(steps: u64) -> Duration {
        Duration::from_millis(steps)
    }

    #[inline]
    #[allow(clippy::cast_possible_truncation)]
    fn whole_steps(duration: &Duration) -> u64 {
        duration.as_millis() as u64
    }
}

/// Second resolution adapter.
#[derive(Debug)]
pub enum SecondResolution {}

impl Resolution for SecondResolution {
    const MAX_DURATION: Duration = Duration::from_secs(u32::MAX as u64);

    #[allow(clippy::cast_possible_truncation)]
    #[inline]
    fn cycle_steps(duration: &Duration, upper_bound: bool) -> u32 {
        let mut steps = duration.as_secs() as u32;
        if upper_bound && duration.subsec_nanos() > 0 {
            steps = steps.saturating_add(1);
        }
        steps
    }

    #[inline]
    fn steps_as_duration(steps: u64) -> Duration {
        Duration::from_secs(steps)
    }

    #[inline]
    fn whole_steps(duration: &Duration) -> u64 {
        duration.as_secs()
    }
}
