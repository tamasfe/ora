//! A timer implementation that supports both blocking and async execution.

use core::{fmt::Debug, time::Duration};

use minstant::Instant;

use crate::{
    resolution::{Milliseconds, Resolution},
    wheel::TimingWheel,
};

/// A timer that runs in a loop and expires tasks.
#[must_use]
pub struct Timer<T, R = Milliseconds>
where
    R: Resolution,
{
    wheel: TimingWheel<T, R>,
    handle: TimerHandle<T>,
    timer_events: tokio::sync::mpsc::UnboundedReceiver<TimerEvent<T>>,
    ready_entries: tokio::sync::mpsc::UnboundedSender<T>,
}

impl<T, R> Timer<T, R>
where
    R: Resolution,
    T: Debug,
{
    /// Create a new timer.
    pub fn new() -> (Self, tokio::sync::mpsc::UnboundedReceiver<T>) {
        let (timer_events_send, timer_events_recv) = tokio::sync::mpsc::unbounded_channel();
        let (ready_entries_send, ready_entries_recv) = tokio::sync::mpsc::unbounded_channel();

        (
            Self {
                wheel: TimingWheel::new(),
                handle: TimerHandle {
                    timer_events: timer_events_send,
                },
                timer_events: timer_events_recv,
                ready_entries: ready_entries_send,
            },
            ready_entries_recv,
        )
    }

    /// Get a handle to this timer.
    pub fn handle(&self) -> TimerHandle<T> {
        self.handle.clone()
    }

    /// Run the timer in a future.
    #[cfg(feature = "async")]
    pub async fn run(mut self) {
        use tokio::sync::mpsc::error::TryRecvError;
        use tracing::Instrument;

        drop(self.handle);
        let ready_entries = self.ready_entries;
        let mut timer_events = self.timer_events;
        let mut state = TimerState::default();

        loop {
            let span = tracing::trace_span!("timer_loop");
            let should_stop = async {
                let steps = state.elapsed_steps::<R>();

                for _ in 0..steps {
                    for entry in self.wheel.tick() {
                        tracing::trace!(?entry, "entry ready");
                        if ready_entries.send(entry).is_err() {
                            return true;
                        }
                    }
                }

                match timer_events.try_recv() {
                    Ok(event) => match event {
                        TimerEvent::Schedule(entry, delay) => {
                            if let Some(entry) = self.wheel.insert(entry, delay) {
                                tracing::trace!(?entry, "entry ready");
                                if ready_entries.send(entry).is_err() {
                                    return true;
                                }
                            }
                        }
                        TimerEvent::Stop => return true,
                    },
                    Err(error) => match error {
                        TryRecvError::Empty => {
                            if self.wheel.is_empty() {
                                match timer_events.recv().await {
                                    Some(event) => {
                                        match event {
                                            TimerEvent::Schedule(entry, delay) => {
                                                // The wheel was empty, it's safe to count from the beginning.
                                                state.reset();
                                                if let Some(entry) = self.wheel.insert(entry, delay) {
                                                    tracing::trace!(?entry, "entry ready");
                                                    if ready_entries.send(entry).is_err() {
                                                        return true;
                                                    }
                                                }
                                            }
                                            TimerEvent::Stop => return true,
                                        }
                                    }
                                    None => return true,
                                }
                            } else {
                                let can_skip = self.wheel.can_skip();

                                #[allow(clippy::cast_lossless)]
                                if can_skip > 0 {
                                    let wait_duration = R::steps_as_duration(can_skip as _);
                                    if wait_duration > Duration::from_millis(5) {
                                        tokio::select! {
                                            event = timer_events.recv() => {
                                                match event {
                                                    Some(event) => match event {
                                                        TimerEvent::Schedule(entry, delay) => {
                                                            if let Some(entry) = self.wheel.insert(entry, delay) {
                                                                tracing::trace!(?entry, "entry ready");
                                                                if ready_entries.send(entry).is_err() {
                                                                    return true;
                                                                }
                                                            }
                                                        }
                                                        TimerEvent::Stop => return true,
                                                    },
                                                    None => return true,
                                                }
                                            }
                                            _ = tokio::time::sleep(wait_duration) => {}
                                        }
                                    } else {
                                        tokio::task::yield_now().await;
                                    }
                                } else {
                                    tokio::task::yield_now().await;
                                }
                            }
                        }
                        TryRecvError::Disconnected => return true,
                    },
                }

                false
            }
            .instrument(span)
            .await;

            if should_stop {
                tracing::trace!("stopping");
                return;
            }
        }
    }
}

/// A handle to a timer that can be used to stop
/// the timer on demand or schedule tasks.
#[derive(Debug)]
#[must_use]
pub struct TimerHandle<T> {
    timer_events: tokio::sync::mpsc::UnboundedSender<TimerEvent<T>>,
}

impl<T> TimerHandle<T> {
    /// Schedule a task in the timer.
    ///
    /// No-op if the timer is not running.
    pub fn schedule(&self, entry: T, delay: Duration) {
        let _ = self.timer_events.send(TimerEvent::Schedule(entry, delay));
    }

    /// Stop the timer.
    ///
    /// No-op if the timer is not running.
    pub fn stop(&self) {
        let _ = self.timer_events.send(TimerEvent::Stop);
    }

    /// Returns whether the timer is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.timer_events.is_closed()
    }
}

impl<T> Clone for TimerHandle<T> {
    fn clone(&self) -> Self {
        Self {
            timer_events: self.timer_events.clone(),
        }
    }
}

enum TimerEvent<T> {
    Schedule(T, Duration),
    Stop,
}

struct TimerState {
    start: Instant,
    total_elapsed: Duration,
    total_elapsed_steps: u128,
}

impl Default for TimerState {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            total_elapsed: Duration::default(),
            total_elapsed_steps: Default::default(),
        }
    }
}

impl TimerState {
    fn reset(&mut self) {
        self.start = Instant::now();
        self.total_elapsed = Duration::ZERO;
        self.total_elapsed_steps = 0;
    }

    fn elapsed_steps<R: Resolution>(&mut self) -> u128 {
        self.total_elapsed = self.start.elapsed();
        let steps = R::whole_steps(&self.total_elapsed);
        let new_steps = steps - self.total_elapsed_steps;
        self.total_elapsed_steps = steps;
        new_steps
    }
}
