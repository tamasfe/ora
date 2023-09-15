//! Scheduler implementation.

use core::pin::pin;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ahash::{AHashMap, HashSet};
use futures::TryStreamExt;
use ora_common::{schedule::NewTask, timeout::TimeoutPolicy, UnixNanos};
use ora_timer::{resolution::Milliseconds, Timer, TimerHandle};
use ora_util::schedule::next_schedule_task;
use thiserror::Error;
use tokio::select;
use uuid::Uuid;

use crate::store::{
    schedule::{SchedulerScheduleStore, SchedulerScheduleStoreEvent},
    task::{ActiveTask, SchedulerTaskStore, SchedulerTaskStoreEvent},
};

/// A scheduler that has a purpose of marking
/// tasks as ready when their target timestamp is reached.
///
/// It also manages spawning new tasks of schedules if needed.
pub struct Scheduler<S> {
    store: S,
}

impl<S> Scheduler<S>
where
    S: SchedulerTaskStore + SchedulerScheduleStore,
{
    /// Create a new scheduler with the given backing store.
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Run the scheduler indefinitely or until a store error occurs.
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn run(self) -> Result<(), Error> {
        let schedule_manager = ScheduleManager::new(&self.store);
        let mut schedule_manager_task = pin!(schedule_manager.run());

        let mut events = pin!(SchedulerTaskStore::events(&self.store)
            .await
            .map_err(store_error)?);
        let pending_tasks = self.store.pending_tasks().await.map_err(store_error)?;

        // Used for cancellations and to prevent accidental duplications.
        let mut scheduled_tasks: AHashMap<Uuid, ScheduledTask> = AHashMap::new();

        let (timer, mut ready_entries) = Timer::<TimerEntry, Milliseconds>::new();
        let timer_handle = timer.handle();

        let mut timer_fut = pin!(timer.run());

        for task in pending_tasks {
            handle_event(
                SchedulerTaskStoreEvent::TaskAdded(task),
                &timer_handle,
                &mut scheduled_tasks,
            );
        }

        // Schedule timeouts for existing tasks,
        // this is done once at the beginning so no deduplication
        // is done.
        let active_tasks = self
            .store
            .active_tasks()
            .await
            .map_err(store_error)?;

        for task in active_tasks {
            schedule_timeout(task, &timer_handle);
        }

        loop {
            select! {
                _ = &mut timer_fut => {
                    panic!("unexpected end of the timer loop");
                }
                event = events.try_next() => {
                    match event {
                        Ok(event) => {
                            match event {
                                Some(event) => {
                                    handle_event(event, &timer_handle, &mut scheduled_tasks);
                                }
                                None => {
                                    return Err(Error::UnexpectedEventStreamEnd);
                                }
                            }
                        }
                        Err(error) => {
                            return Err(store_error(error));
                        }
                    }
                }
                timer_entry = ready_entries.recv() => {
                    let timer_entry = timer_entry.unwrap();
                    match timer_entry {
                        TimerEntry::TaskReady(task_id) => {
                            let state = scheduled_tasks.remove(&task_id).unwrap();
                            tracing::trace!(%task_id, "task ready");
                            if !state.cancelled {
                                self.store.task_ready(task_id).await.map_err(store_error)?;
                            }
                        }
                        TimerEntry::TaskTimeout(task_id) => {
                            self.store.task_timed_out(task_id).await.map_err(store_error)?;
                        }
                    }
                }
                manager_result = &mut schedule_manager_task => {
                    manager_result?;
                    unreachable!()
                }
            }
        }
    }
}

#[tracing::instrument(level = "trace", skip_all)]
fn handle_event(
    event: SchedulerTaskStoreEvent,
    timer: &TimerHandle<TimerEntry>,
    scheduled_tasks: &mut AHashMap<Uuid, ScheduledTask>,
) {
    match event {
        SchedulerTaskStoreEvent::TaskAdded(task) => {
            if scheduled_tasks.contains_key(&task.id) {
                tracing::debug!(task_id = %task.id, "task already scheduled");
                return;
            }
            let task_unix = Duration::from_nanos(task.target.0);

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time cannot be before unix epoch");

            let task_delay = task_unix.saturating_sub(now);

            tracing::trace!(task_id = %task.id, "task scheduled");
            scheduled_tasks.insert(task.id, ScheduledTask::default());
            timer.schedule(TimerEntry::TaskReady(task.id), task_delay);
            schedule_timeout(task.into(), timer);
        }
        SchedulerTaskStoreEvent::TaskCancelled(task_id) => {
            if let Some(task) = scheduled_tasks.get_mut(&task_id) {
                if task.cancelled {
                    tracing::debug!(%task_id, "task already cancelled");
                }
                tracing::trace!(%task_id, "task cancelled");
                task.cancelled = true;
            } else {
                tracing::debug!(%task_id, "task was cancelled but it was not scheduled");
            }
        }
    }
}

#[tracing::instrument(level = "trace", skip_all)]
fn schedule_timeout(task: ActiveTask, timer: &TimerHandle<TimerEntry>) {
    match task.timeout {
        TimeoutPolicy::Never => {}
        TimeoutPolicy::FromTarget { timeout } => {
            let task_unix = Duration::from_nanos(task.target.0);

            let timeout_unix: Duration = match Duration::try_from(timeout) {
                Ok(t) => t + task_unix,
                Err(error) => {
                    tracing::warn!(%error, "timeout out of range");
                    return;
                }
            };

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time cannot be before unix epoch");

            let timeout_delay = timeout_unix.saturating_sub(now);

            timer.schedule(TimerEntry::TaskTimeout(task.id), timeout_delay);
        }
    }
}

/// A scheduler error.
#[derive(Debug, Error)]
pub enum Error {
    /// The store event stream ended unexpectedly.
    #[error("unexpected end of event stream")]
    UnexpectedEventStreamEnd,
    /// A store error ocurred.
    #[error("store error: {0:?}")]
    Store(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Default)]
struct ScheduledTask {
    cancelled: bool,
}

#[derive(Debug)]
enum TimerEntry {
    TaskReady(Uuid),
    TaskTimeout(Uuid),
}

struct ScheduleManager<'s, S>
where
    S: SchedulerScheduleStore,
{
    store: &'s S,
    active_schedules: HashSet<Uuid>,
}

impl<'s, S> ScheduleManager<'s, S>
where
    S: SchedulerScheduleStore,
{
    fn new(store: &'s S) -> Self {
        Self {
            store,
            active_schedules: HashSet::default(),
        }
    }

    async fn run(mut self) -> Result<(), Error> {
        let mut events = pin!(self.store.events().await.map_err(store_error)?);

        let pending_schedules = self.store.pending_schedules().await.map_err(store_error)?;

        for schedule in pending_schedules {
            self.handle_event(SchedulerScheduleStoreEvent::ScheduleAdded(Box::new(
                schedule,
            )))
            .await?;
        }

        while let Some(event) = events.try_next().await.map_err(store_error)? {
            self.handle_event(event).await?;
        }

        Err(Error::UnexpectedEventStreamEnd)
    }

    async fn handle_event(&mut self, event: SchedulerScheduleStoreEvent) -> Result<(), Error> {
        match event {
            SchedulerScheduleStoreEvent::ScheduleAdded(schedule) => {
                if self.active_schedules.contains(&schedule.id) {
                    tracing::debug!("active schedule already exists");
                    return Ok(());
                }
                self.active_schedules.insert(schedule.id);

                let next_target = next_schedule_task(&schedule.definition, None, UnixNanos::now());

                if let Some(next_target) = next_target {
                    match &schedule.definition.new_task {
                        NewTask::Repeat { task } => {
                            self.store
                                .add_task(schedule.id, task.clone().at_unix(next_target))
                                .await
                                .map_err(store_error)?;
                        }
                    }
                }
            }
            SchedulerScheduleStoreEvent::TaskFinished(task_id) => {
                if let Some(schedule) = self
                    .store
                    .pending_schedule_of_task(task_id)
                    .await
                    .map_err(store_error)?
                {
                    self.active_schedules.insert(schedule.id);
                    let prev_target = self.store.task_target(task_id).await.map_err(store_error)?;
                    let next_target = next_schedule_task(
                        &schedule.definition,
                        Some(prev_target),
                        UnixNanos::now(),
                    );

                    if let Some(next_target) = next_target {
                        match &schedule.definition.new_task {
                            NewTask::Repeat { task } => {
                                self.store
                                    .add_task(schedule.id, task.clone().at_unix(next_target))
                                    .await
                                    .map_err(store_error)?;
                            }
                        }
                    }
                }
            }
            SchedulerScheduleStoreEvent::ScheduleCancelled(schedule_id) => {
                self.active_schedules.remove(&schedule_id);
            }
        }

        Ok(())
    }
}

fn store_error<E: std::error::Error + Send + Sync + 'static>(error: E) -> Error {
    Error::Store(Box::new(error))
}
