//! Low-level timer implementations.

use std::{cmp::Reverse, collections::BinaryHeap, thread, time::Duration};

use minstant::Instant;
use resolution::{MillisecondResolution, Resolution};

extern crate alloc;

pub mod resolution;
pub mod wheel;

/// A delayed item with a delay duration
/// and an arbitrary payload.
pub struct Delayed<T>(T, Duration);

impl<T> Delayed<T> {
    /// Create a new delayed item.
    pub fn new(item: T, delay: Duration) -> Self {
        Self(item, delay)
    }
}

impl<T> PartialEq for Delayed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.1 == other.1
    }
}

impl<T> Eq for Delayed<T> {}

impl<T> PartialOrd for Delayed<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.1.cmp(&other.1))
    }
}

impl<T> Ord for Delayed<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.1.cmp(&other.1)
    }
}

/// Options for the timer loop.
#[derive(Debug, Clone, Copy)]
pub struct TimerOptions {
    /// The threshold to the next tick at which the timer loop
    /// will sleep to reduce CPU usage.
    ///
    /// Set to `Duration::ZERO` to always keep
    /// the timer in a busy loop achieving
    /// the lowest possible latency at the
    /// cost of high CPU usage.
    pub sleep_threshold: Duration,
    /// The rate at which bookkeeping is run
    /// while the timer loop is idle or waiting
    /// for the next tick.
    pub bookkeeping_interval: Duration,
}

impl Default for TimerOptions {
    fn default() -> Self {
        Self {
            sleep_threshold: Duration::from_millis(20),
            bookkeeping_interval: Duration::from_millis(500),
        }
    }
}

/// Run a timer loop with a hierarchical timing wheel
/// algorithm and the given resolution.
///
/// The precision of the timer loop is constrained by the
/// resolution used (among other factors) but generally
/// provides predictable latency even for large
/// numbers of scheduled jobs.
#[allow(clippy::cast_possible_truncation)]
pub fn run_hierarchical_timer<T, R: Resolution>(
    options: TimerOptions,
    mut callback: impl FnMut(&mut Vec<Delayed<T>>, &mut Vec<T>) -> TimerLoopAction,
) {
    let mut wheel = wheel::TimingWheel::<T, R>::new();
    let TimerOptions {
        sleep_threshold,
        bookkeeping_interval,
    } = options;

    macro_rules! run_callback {
        ($wheel:tt, $new_jobs:tt, $ready_jobs:tt) => {{
            let action = callback(&mut $new_jobs, &mut $ready_jobs);
            $ready_jobs.clear();
            let got_new_jobs = !$new_jobs.is_empty();
            if got_new_jobs {
                for new in $new_jobs.drain(..) {
                    if let Some(t) = $wheel.insert(new.0, new.1) {
                        $ready_jobs.push(t);
                    }
                }
            }

            match action {
                TimerLoopAction::Continue => {}
                TimerLoopAction::Stop => {
                    return;
                }
                TimerLoopAction::StopWhenIdle => {
                    if $wheel.is_empty() {
                        return;
                    }
                }
            }

            got_new_jobs
        }};
    }

    let mut last_tick = Instant::now();
    let mut new_jobs = Vec::<Delayed<T>>::new();
    let mut ready_jobs = Vec::new();
    loop {
        run_callback!(wheel, new_jobs, ready_jobs);

        let now = Instant::now();
        let elapsed = now - last_tick;
        let elapsed_steps = R::whole_steps(&elapsed);

        if elapsed_steps == 0 {
            continue;
        }

        let mut can_skip_steps = wheel.can_skip();
        can_skip_steps = can_skip_steps.min(elapsed_steps as u32);

        if can_skip_steps > 0 {
            wheel.skip(can_skip_steps);
        }

        let tick_steps = elapsed_steps - u64::from(can_skip_steps);

        for _ in 0..tick_steps {
            wheel.tick_with(&mut ready_jobs);
        }
        wheel.gc(0xF_FFFF);

        last_tick = now;

        if wheel.is_empty() {
            thread::sleep(bookkeeping_interval);
            continue;
        }

        let can_skip_steps = wheel.can_skip();
        let sleep_delay = MillisecondResolution::steps_as_duration(u64::from(can_skip_steps));

        // We continue the timer loop earlier than the next tick
        // to reduce the chance of skipping a tick.
        let mut wait_duration = sleep_delay / 2;

        loop {
            let got_new_jobs = run_callback!(wheel, new_jobs, ready_jobs);
            if got_new_jobs {
                continue;
            }

            if sleep_threshold == Duration::ZERO
                || wait_duration == Duration::ZERO
                || bookkeeping_interval == Duration::ZERO
                || wait_duration < sleep_threshold
            {
                break;
            }

            let poll_duration = wait_duration.min(bookkeeping_interval);

            thread::sleep(poll_duration);
            wait_duration -= poll_duration;
        }
    }
}

/// Run a timer loop backed by a binary heap structure.
///
/// The precision of the timer loop is not constrained
/// by the algorithm itself but by the resolution of the
/// time source.
///
/// This algorithm also has lower bookkeeping overhead
/// when it is not contended and offers a very low minimum latency
/// however performance degrades as the number of scheduled jobs increases.
pub fn run_binary_heap_timer<T>(
    options: TimerOptions,
    mut callback: impl FnMut(&mut Vec<Delayed<T>>, &mut Vec<T>) -> TimerLoopAction,
) {
    let mut heap: BinaryHeap<Reverse<Delayed<T>>> = BinaryHeap::new();
    let TimerOptions {
        sleep_threshold,
        bookkeeping_interval,
    } = options;

    macro_rules! run_callback {
        ($heap:tt, $new_jobs:tt, $ready_jobs:tt, $elapsed:tt) => {{
            let action = callback(&mut $new_jobs, &mut $ready_jobs);
            $ready_jobs.clear();
            let got_new_jobs = !$new_jobs.is_empty();
            if got_new_jobs {
                for mut new in $new_jobs.drain(..) {
                    if new.1 == Duration::ZERO {
                        $ready_jobs.push(new.0);
                        continue;
                    }

                    new.1 += $elapsed;
                    $heap.push(Reverse(new));
                }
            }

            match action {
                TimerLoopAction::Continue => {}
                TimerLoopAction::Stop => {
                    return;
                }
                TimerLoopAction::StopWhenIdle => {
                    if $heap.is_empty() {
                        return;
                    }
                }
            }

            got_new_jobs
        }};
    }

    let start = Instant::now();
    let mut elapsed = Duration::ZERO;
    let mut new_jobs = Vec::<Delayed<T>>::new();
    let mut ready_jobs = Vec::new();

    loop {
        run_callback!(heap, new_jobs, ready_jobs, elapsed);

        let now = Instant::now();
        elapsed = now - start;

        while let Some(Reverse(job)) = heap.peek() {
            if job.1 <= elapsed {
                let Reverse(job) = unsafe { heap.pop().unwrap_unchecked() };
                ready_jobs.push(job.0);
            } else {
                break;
            }
        }

        if heap.is_empty() {
            thread::sleep(bookkeeping_interval);
            continue;
        }

        let sleep_delay = heap
            .peek()
            .map(|Reverse(job)| job.1 - elapsed)
            .unwrap_or_default();

        // We continue the timer loop earlier than the next tick
        // to reduce the chance of skipping a tick.
        let mut wait_duration = sleep_delay / 2;

        loop {
            let got_new_jobs = run_callback!(heap, new_jobs, ready_jobs, elapsed);
            if got_new_jobs {
                continue;
            }

            if wait_duration == Duration::ZERO
                || bookkeeping_interval == Duration::ZERO
                || wait_duration <= sleep_threshold
            {
                break;
            }

            let poll_duration = wait_duration.min(bookkeeping_interval);

            thread::sleep(poll_duration);
            wait_duration -= poll_duration;
        }
    }
}

/// A desired action to be taken by the timer loop.
#[must_use]
pub enum TimerLoopAction {
    /// Continue the timer loop.
    Continue,
    /// Stop the timer loop immediately.
    Stop,
    /// Stop the timer loop when it becomes idle.
    ///
    /// Note that this does not prevent new jobs from being added,
    /// and the timer loop will continue to run until it becomes idle.
    StopWhenIdle,
}
