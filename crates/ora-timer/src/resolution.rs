//! Resolution adapters for timers.

use core::time::Duration;

/// A timer resolution.
///
/// A coarser resolution might improve scheduling performance
/// of tasks where delays would exceed the `2^32` steps with finer resolutions.
///
/// Note that with the current design it's not possible
/// to know when the earliest next task is scheduled,
/// only which buckets are empty, thus resource saving
/// optimizations can only take buckets into account.
/// E.g. using seconds as a resolution will always
/// cause a busy wait as long as there is a task
/// that will expire within the next `255` seconds.
pub trait Resolution {
    /// The maximum duration that can be represented as [`u32`]
    /// at this resolution.
    const MAX_DURATION: Duration;

    /// Convert the given duration into timer cycle steps.
    /// The given duration is guaranteed to be smaller than [`Resolution::MAX_DURATION`].
    ///
    /// If `upper_bound` is set, the returned value should be rounded up.
    fn cycle_steps(duration: &Duration, upper_bound: bool) -> u32;

    /// Return total steps required for the given duration.
    /// The returned steps should not be rounded up.
    fn whole_steps(duration: &Duration) -> u128;

    /// Convert the given steps to a duration.
    fn steps_as_duration(steps: u64) -> Duration;
}

/// Microsecond resolution adapter.
///
/// Note that this kind of resolution is only
/// possible in very specific setups, as even a context switch
/// or allocation takes longer on Linux.
pub enum Microseconds {}

impl Resolution for Microseconds {
    const MAX_DURATION: Duration = Duration::from_micros(u32::MAX as u64);

    #[allow(clippy::cast_possible_truncation)]
    fn cycle_steps(duration: &Duration, upper_bound: bool) -> u32 {
        let mut steps = (duration.as_secs() * 1_000_000) as u32;
        let micros = duration.subsec_micros();
        steps += micros;
        if upper_bound && duration.subsec_nanos() % 1_000 > 0 {
            steps = steps.saturating_add(1);
        }
        steps
    }

    fn steps_as_duration(steps: u64) -> Duration {
        Duration::from_micros(steps)
    }

    fn whole_steps(duration: &Duration) -> u128 {
        duration.as_nanos() / 1_000
    }
}

/// Millisecond resolution adapter.
///
/// The default resolution for `ora`
/// that can be achieved most of the time.
pub enum Milliseconds {}

impl Resolution for Milliseconds {
    const MAX_DURATION: Duration = Duration::from_millis(u32::MAX as u64);

    #[allow(clippy::cast_possible_truncation)]
    fn cycle_steps(duration: &Duration, upper_bound: bool) -> u32 {
        let mut steps = (duration.as_secs() * 1000) as u32;
        let ms = duration.subsec_millis();
        steps += ms;

        if upper_bound && duration.subsec_nanos() % 1_000_000 > 0 {
            steps = steps.saturating_add(1);
        }

        steps
    }

    fn steps_as_duration(steps: u64) -> Duration {
        Duration::from_millis(steps)
    }

    fn whole_steps(duration: &Duration) -> u128 {
        duration.as_nanos() / 1_000_000
    }
}

/// Second resolution adapter.
///
/// Use this if the use-case involves delays often exceeding `2^32`
/// milliseconds (approx. 50 days) and you don't need millisecond granularity.
pub enum Seconds {}

impl Resolution for Seconds {
    const MAX_DURATION: Duration = Duration::from_secs(u32::MAX as u64);

    #[allow(clippy::cast_possible_truncation)]
    fn cycle_steps(duration: &Duration, upper_bound: bool) -> u32 {
        let mut steps = duration.as_secs() as u32;
        if upper_bound && duration.subsec_nanos() > 0 {
            steps = steps.saturating_add(1);
        }
        steps
    }

    fn steps_as_duration(steps: u64) -> Duration {
        Duration::from_secs(steps)
    }

    fn whole_steps(duration: &Duration) -> u128 {
        duration.as_nanos() / 1_000_000_000
    }
}
