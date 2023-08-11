//! A hierarchical timing wheel based on <https://github.com/Bathtor/rust-hash-wheel-timer>.

mod byte;

use alloc::{boxed::Box, vec::Vec};
use core::{marker::PhantomData, mem, time::Duration};

use byte::ByteWheel;

use crate::resolution::{Milliseconds, Resolution};

/// A hierarchical timing wheel with a given entry type and resolution.
#[must_use]
pub struct TimingWheel<T, R = Milliseconds>
where
    R: Resolution,
{
    primary: Box<ByteWheel<T, [u8; 0]>>,
    secondary: Box<ByteWheel<T, [u8; 1]>>,
    tertiary: Box<ByteWheel<T, [u8; 2]>>,
    quarternary: Box<ByteWheel<T, [u8; 3]>>,
    overflow: Vec<OverflowEntry<T>>,
    _resolution: PhantomData<R>,
}

impl<T, R> Default for TimingWheel<T, R>
where
    R: Resolution,
{
    fn default() -> Self {
        TimingWheel::new()
    }
}

impl<T, R> TimingWheel<T, R>
where
    R: Resolution,
{
    /// Create a new timing wheel.
    pub fn new() -> Self {
        TimingWheel {
            primary: Box::new(ByteWheel::new()),
            secondary: Box::new(ByteWheel::new()),
            tertiary: Box::new(ByteWheel::new()),
            quarternary: Box::new(ByteWheel::new()),
            overflow: Vec::new(),
            _resolution: PhantomData,
        }
    }

    /// Returns the entry if it has already expired.
    #[allow(clippy::cast_possible_truncation)]
    #[tracing::instrument(level = "trace", skip_all)]
    pub fn insert(&mut self, entry: T, delay: Duration) -> Option<T> {
        if delay >= R::MAX_DURATION {
            let remaining_delay = R::steps_as_duration(self.remaining_time_in_cycle());
            let new_delay = delay - remaining_delay;
            let overflow_e = OverflowEntry::new(entry, new_delay);
            self.overflow.push(overflow_e);
            None
        } else {
            let delay = R::cycle_steps(&delay, true);
            let current_time = self.cycle_timestamp();
            let absolute_time = delay.wrapping_add(current_time);
            let absolute_bytes: [u8; 4] = absolute_time.to_be_bytes();
            let zero_time = absolute_time ^ current_time; // a-b%2
            let zero_bytes: [u8; 4] = zero_time.to_be_bytes();
            match zero_bytes {
                [0, 0, 0, 0] => Some(entry),
                [0, 0, 0, _] => {
                    self.primary.insert(absolute_bytes[3], entry, []);
                    None
                }
                [0, 0, _, _] => {
                    self.secondary
                        .insert(absolute_bytes[2], entry, [absolute_bytes[3]]);
                    None
                }
                [0, _, _, _] => {
                    self.tertiary.insert(
                        absolute_bytes[1],
                        entry,
                        [absolute_bytes[2], absolute_bytes[3]],
                    );
                    None
                }
                [_, _, _, _] => {
                    self.quarternary.insert(
                        absolute_bytes[0],
                        entry,
                        [absolute_bytes[1], absolute_bytes[2], absolute_bytes[3]],
                    );
                    None
                }
            }
        }
    }

    /// Advance the timing wheel and collect all entries that have been expired.
    #[tracing::instrument(level = "trace", skip_all)]
    pub fn tick(&mut self) -> Vec<T> {
        let mut res: Vec<T> = Vec::new();
        // primary
        let (move0, current0) = self.primary.tick();
        res.extend(move0.map(|we| we.entry));
        if current0 == 0u8 {
            // secondary
            let (move1, current1) = self.secondary.tick();
            // Don't bother reserving, as most of the values will likely be redistributed over the primary wheel instead of being returned
            for we in move1 {
                if we.rest[0] == 0u8 {
                    res.push(we.entry);
                } else {
                    self.primary.insert(we.rest[0], we.entry, []);
                }
            }
            if current1 == 0u8 {
                // tertiary
                let (move2, current2) = self.tertiary.tick();
                for we in move2 {
                    match we.rest {
                        [0, 0] => {
                            res.push(we.entry);
                        }
                        [0, b0] => {
                            self.primary.insert(b0, we.entry, []);
                        }
                        [b1, b0] => {
                            self.secondary.insert(b1, we.entry, [b0]);
                        }
                    }
                }
                if current2 == 0u8 {
                    // quarternary
                    let (move3, current3) = self.quarternary.tick();
                    for we in move3 {
                        match we.rest {
                            [0, 0, 0] => {
                                res.push(we.entry);
                            }
                            [0, 0, b0] => {
                                self.primary.insert(b0, we.entry, []);
                            }
                            [0, b1, b0] => {
                                self.secondary.insert(b1, we.entry, [b0]);
                            }
                            [b2, b1, b0] => {
                                self.tertiary.insert(b2, we.entry, [b1, b0]);
                            }
                        }
                    }
                    if current3 == 0u8 {
                        // overflow list
                        if !self.overflow.is_empty() {
                            // assume that about half are going to be scheduled now
                            let mut ol: Vec<OverflowEntry<T>> =
                                Vec::with_capacity(self.overflow.len() / 2);
                            mem::swap(&mut self.overflow, &mut ol);
                            for overflow_e in ol {
                                if let Some(entry) =
                                    self.insert(overflow_e.entry, overflow_e.remaining_delay)
                                {
                                    res.push(entry);
                                }
                            }
                        }
                    }
                }
            }
        }
        res
    }

    /// Skip `amount` steps, note that this will succeed
    /// and no checks will take place.
    ///
    /// Use [`TimingWheel::can_skip`] to determine if this function
    /// can be used without silently dropping any entries that
    /// have not been expired.
    #[tracing::instrument(level = "trace", skip_all)]
    pub fn skip(&mut self, amount: u32) {
        let new_time = self.cycle_timestamp().wrapping_add(amount);
        let new_time_bytes: [u8; 4] = new_time.to_be_bytes();
        self.primary.set_current(new_time_bytes[3]);
        self.secondary.set_current(new_time_bytes[2]);
        self.tertiary.set_current(new_time_bytes[1]);
        self.quarternary.set_current(new_time_bytes[0]);
    }

    /// Returns how many steps can be skipped safely without
    /// missing entries.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_lossless)]
    #[tracing::instrument(level = "trace", skip_all)]
    pub fn can_skip(&self) -> u32 {
        if self.primary.is_empty() {
            if self.secondary.is_empty() {
                if self.tertiary.is_empty() {
                    if self.quarternary.is_empty() {
                        if self.overflow.is_empty() {
                            0
                        } else {
                            (self.remaining_time_in_cycle() - 1u64) as u32
                        }
                    } else {
                        let tertiary_current = self.cycle_timestamp() & (TERTIARY_LENGTH - 1u32);
                        let rem = TERTIARY_LENGTH - tertiary_current;
                        rem - 1u32
                    }
                } else {
                    let secondary_current = self.cycle_timestamp() & (SECONDARY_LENGTH - 1u32);
                    let rem = SECONDARY_LENGTH - secondary_current;
                    rem - 1u32
                }
            } else {
                let primary_current = self.primary.current() as u32;
                let rem = PRIMARY_LENGTH - primary_current;
                rem - 1u32
            }
        } else {
            0
        }
    }

    /// Return the amount of entries in the wheel.
    #[must_use]
    pub fn len(&self) -> usize {
        self.primary.len()
            + self.secondary.len()
            + self.tertiary.len()
            + self.quarternary.len()
            + self.overflow.len()
    }

    /// Return whether the wheel is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[allow(clippy::cast_lossless)]
    fn remaining_time_in_cycle(&self) -> u64 {
        CYCLE_LENGTH - (self.cycle_timestamp() as u64)
    }

    #[must_use]
    fn cycle_timestamp(&self) -> u32 {
        let time_bytes = [
            self.quarternary.current(),
            self.tertiary.current(),
            self.secondary.current(),
            self.primary.current(),
        ];
        u32::from_be_bytes(time_bytes)
    }
}

const CYCLE_LENGTH: u64 = 1 << 32; // 2^32
const PRIMARY_LENGTH: u32 = 1 << 8; // 2^8
const SECONDARY_LENGTH: u32 = 1 << 16; // 2^16
const TERTIARY_LENGTH: u32 = 1 << 24; // 2^24

struct OverflowEntry<T> {
    entry: T,
    remaining_delay: Duration,
}
impl<T> OverflowEntry<T> {
    fn new(entry: T, remaining_delay: Duration) -> Self {
        OverflowEntry {
            entry,
            remaining_delay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolution::Milliseconds;

    #[test]
    fn smoke_millis() {
        let mut wheel: TimingWheel<usize, Milliseconds> = TimingWheel::new();
        assert!(wheel.insert(0, Duration::ZERO).is_some());

        assert!(wheel.insert(0, Duration::from_millis(1)).is_none());
        assert_eq!(wheel.len(), 1);
        assert_eq!(wheel.tick().pop().unwrap(), 0);

        assert!(wheel.insert(0, Duration::from_millis(10)).is_none());
        assert_eq!(wheel.len(), 1);
        assert_eq!(wheel.can_skip(), 0);
    }

    #[test]
    fn skip_millis() {
        let mut wheel: TimingWheel<usize, Milliseconds> = TimingWheel::new();

        assert!(wheel.insert(0, Duration::from_millis(255)).is_none());
        assert_eq!(wheel.len(), 1);
        assert_eq!(wheel.can_skip(), 0);

        let mut wheel: TimingWheel<usize, Milliseconds> = TimingWheel::new();
        assert!(wheel.insert(0, Duration::from_millis(256)).is_none());
        assert_eq!(wheel.len(), 1);
        assert_eq!(wheel.can_skip(), 255);
        wheel.skip(255);
        assert_eq!(wheel.tick().pop().unwrap(), 0);

        let mut wheel: TimingWheel<usize, Milliseconds> = TimingWheel::new();
        assert!(wheel.insert(0, Duration::from_millis(65536)).is_none());
        assert_eq!(wheel.len(), 1);
        assert_eq!(wheel.can_skip(), 65535);
        wheel.skip(65535);
        assert_eq!(wheel.tick().pop().unwrap(), 0);
    }
}
