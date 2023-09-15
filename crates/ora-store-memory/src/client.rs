#![allow(clippy::cast_possible_truncation)]
use std::{sync::Arc, time::Duration};

use ahash::HashSet;
use async_trait::async_trait;
use memmem::{Searcher, TwoWaySearcher};
use ora_client::{
    ClientOperations, LabelValueMatch, RawTaskResult, ScheduleListOrder, ScheduleOperations,
    Schedules, TaskListOrder, TaskOperations, Tasks,
};
use ora_common::{
    schedule::ScheduleDefinition,
    task::{TaskDataFormat, TaskDefinition, TaskStatus},
    UnixNanos,
};
use ora_scheduler::store::{
    schedule::{ActiveSchedule, SchedulerScheduleStoreEvent},
    task::{PendingTask, SchedulerTaskStoreEvent},
};
use uuid::Uuid;

use crate::{Error, MemoryStore, ScheduleState, TaskState};

#[async_trait]
impl ClientOperations for MemoryStore {
    async fn add_task(&self, task: TaskDefinition) -> eyre::Result<Uuid> {
        let id = Uuid::new_v4();
        let target = task.target;
        let timeout = task.timeout;
        self.inner.tasks.lock().insert(
            id,
            TaskState {
                id,
                status: TaskStatus::Pending,
                definition: task,
                output: None,
                failure_reason: None,
                output_format: TaskDataFormat::default(),
                schedule_id: None,
                added_at: UnixNanos::now(),
                ready_at: None,
                started_at: None,
                succeeded_at: None,
                failed_at: None,
                cancelled_at: None,
                worker_id: None,
            },
        );

        let _ = self
            .scheduler_task_events
            .send(SchedulerTaskStoreEvent::TaskAdded(PendingTask {
                id,
                target,
                timeout,
            }));

        Ok(id)
    }

    async fn task(&self, task_id: Uuid) -> eyre::Result<Arc<dyn TaskOperations>> {
        if !self.inner.tasks.lock().contains_key(&task_id) {
            return Err(Error::TaskNotFound(task_id).into());
        }
        Ok(Arc::new(MemoryStoreTaskOperations {
            store: self.clone(),
            task_id,
        }))
    }

    async fn tasks(&self, options: &Tasks) -> eyre::Result<Vec<Arc<dyn TaskOperations>>> {
        let mut tasks = (*self.inner.tasks.lock()).clone();

        match options.order {
            TaskListOrder::AddedAsc => {
                tasks.sort_by(|_, a, _, b| a.added_at.cmp(&b.added_at));
            }
            TaskListOrder::AddedDesc => {
                tasks.sort_by(|_, a, _, b| b.added_at.cmp(&a.added_at));
            }
            TaskListOrder::TargetAsc => {
                tasks.sort_by(|_, a, _, b| a.definition.target.cmp(&b.definition.target));
            }
            TaskListOrder::TargetDesc => {
                tasks.sort_by(|_, a, _, b| b.definition.target.cmp(&a.definition.target));
            }
        }

        Ok(tasks
            .values()
            .filter(filter_tasks(options))
            .map(|task| {
                Arc::new(MemoryStoreTaskOperations {
                    store: self.clone(),
                    task_id: task.id,
                }) as Arc<_>
            })
            .skip(options.offset as _)
            .take(options.limit as _)
            .collect())
    }

    async fn task_count(&self, options: &Tasks) -> eyre::Result<u64> {
        Ok(self
            .inner
            .tasks
            .lock()
            .values()
            .filter(filter_tasks(options))
            .count() as u64)
    }

    async fn task_labels(&self) -> eyre::Result<Vec<String>> {
        Ok(self
            .inner
            .tasks
            .lock()
            .values()
            .fold(HashSet::default(), |mut keys, task| {
                keys.extend(task.definition.labels.keys().cloned());
                keys
            })
            .into_iter()
            .collect())
    }

    async fn task_kinds(&self) -> eyre::Result<Vec<String>> {
        Ok(self
            .inner
            .tasks
            .lock()
            .values()
            .fold(HashSet::default(), |mut keys, task| {
                if !keys.contains(task.definition.worker_selector.kind.as_ref()) {
                    keys.insert(task.definition.worker_selector.kind.to_string());
                }
                keys
            })
            .into_iter()
            .collect())
    }

    async fn add_schedule(&self, schedule: ScheduleDefinition) -> eyre::Result<Uuid> {
        let id = Uuid::new_v4();
        self.inner.schedules.lock().insert(
            id,
            ScheduleState {
                id,
                definition: schedule.clone(),
                active: true,
                active_task: None,
                added_at: UnixNanos::now(),
                cancelled_at: None,
            },
        );

        let _ = self
            .scheduler_schedule_events
            .send(SchedulerScheduleStoreEvent::ScheduleAdded(Box::new(
                ActiveSchedule {
                    id,
                    definition: schedule,
                },
            )));

        Ok(id)
    }

    async fn schedule(&self, schedule_id: Uuid) -> eyre::Result<Arc<dyn ScheduleOperations>> {
        if !self.inner.schedules.lock().contains_key(&schedule_id) {
            return Err(Error::ScheduleNotFound(schedule_id).into());
        }
        Ok(Arc::new(MemoryStoreScheduleOperations {
            store: self.clone(),
            schedule_id,
        }))
    }

    async fn schedules(
        &self,
        options: &ora_client::Schedules,
    ) -> eyre::Result<Vec<Arc<dyn ScheduleOperations>>> {
        let mut schedules = (*self.inner.schedules.lock()).clone();

        match options.order {
            ScheduleListOrder::AddedAsc => {
                schedules.sort_by(|_, a, _, b| a.added_at.cmp(&b.added_at));
            }
            ScheduleListOrder::AddedDesc => {
                schedules.sort_by(|_, a, _, b| b.added_at.cmp(&a.added_at));
            }
        }

        Ok(schedules
            .values()
            .filter(filter_schedules(options))
            .map(|schedule| {
                Arc::new(MemoryStoreScheduleOperations {
                    store: self.clone(),
                    schedule_id: schedule.id,
                }) as Arc<_>
            })
            .skip(options.offset as _)
            .take(options.limit as _)
            .collect())
    }

    async fn schedule_count(&self, options: &ora_client::Schedules) -> eyre::Result<u64> {
        Ok(self
            .inner
            .schedules
            .lock()
            .values()
            .filter(filter_schedules(options))
            .count() as u64)
    }

    async fn schedule_labels(&self) -> eyre::Result<Vec<String>> {
        Ok(self
            .inner
            .schedules
            .lock()
            .values()
            .fold(HashSet::default(), |mut keys, task| {
                keys.extend(task.definition.labels.keys().cloned());
                keys
            })
            .into_iter()
            .collect())
    }
}

#[derive(Debug)]
pub struct MemoryStoreTaskOperations {
    store: MemoryStore,
    task_id: Uuid,
}

#[async_trait]
impl TaskOperations for MemoryStoreTaskOperations {
    fn id(&self) -> Uuid {
        self.task_id
    }

    async fn status(&self) -> eyre::Result<TaskStatus> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .status)
    }

    async fn target(&self) -> eyre::Result<UnixNanos> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .definition
            .target)
    }

    async fn definition(&self) -> eyre::Result<TaskDefinition> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .definition
            .clone())
    }

    async fn result(&self) -> eyre::Result<Option<RawTaskResult>> {
        let tasks = self.store.inner.tasks.lock();
        let task = tasks.get(&self.task_id).unwrap();
        match task.status {
            TaskStatus::Succeeded => Ok(Some(RawTaskResult::Success {
                output_format: task.output_format,
                output: task.output.clone().unwrap(),
            })),
            TaskStatus::Failed => Ok(Some(RawTaskResult::Failure {
                reason: task.failure_reason.clone().unwrap(),
            })),
            TaskStatus::Cancelled => Ok(Some(RawTaskResult::Cancelled)),
            _ => Ok(None),
        }
    }

    async fn wait_result(&self) -> eyre::Result<RawTaskResult> {
        loop {
            if let Some(output) = self.result().await? {
                return Ok(output);
            }

            // FIXME: replace polling with an event
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    async fn schedule(&self) -> eyre::Result<Option<Arc<dyn ScheduleOperations>>> {
        let schedule_id = self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .schedule_id;

        Ok(schedule_id.map(|schedule_id| {
            Arc::new(MemoryStoreScheduleOperations {
                schedule_id,
                store: self.store.clone(),
            }) as Arc<_>
        }))
    }

    async fn added_at(&self) -> eyre::Result<UnixNanos> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .added_at)
    }

    async fn ready_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .ready_at)
    }

    async fn started_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .started_at)
    }

    async fn succeeded_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .succeeded_at)
    }

    async fn failed_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .failed_at)
    }

    async fn cancelled_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .cancelled_at)
    }

    async fn cancel(&self) -> eyre::Result<()> {
        self.store.cancel_task(self.task_id)?;
        Ok(())
    }

    async fn worker_id(&self) -> eyre::Result<Option<Uuid>> {
        Ok(self
            .store
            .inner
            .tasks
            .lock()
            .get(&self.task_id)
            .unwrap()
            .worker_id)
    }
}

#[derive(Debug)]
pub struct MemoryStoreScheduleOperations {
    store: MemoryStore,
    schedule_id: Uuid,
}

#[async_trait]
impl ScheduleOperations for MemoryStoreScheduleOperations {
    fn id(&self) -> Uuid {
        self.schedule_id
    }

    async fn definition(&self) -> eyre::Result<ScheduleDefinition> {
        Ok(self
            .store
            .inner
            .schedules
            .lock()
            .get(&self.schedule_id)
            .unwrap()
            .definition
            .clone())
    }

    async fn is_active(&self) -> eyre::Result<bool> {
        Ok(self
            .store
            .inner
            .schedules
            .lock()
            .get(&self.schedule_id)
            .unwrap()
            .active)
    }

    async fn added_at(&self) -> eyre::Result<UnixNanos> {
        Ok(self
            .store
            .inner
            .schedules
            .lock()
            .get(&self.schedule_id)
            .unwrap()
            .added_at)
    }

    async fn cancelled_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(self
            .store
            .inner
            .schedules
            .lock()
            .get(&self.schedule_id)
            .unwrap()
            .cancelled_at)
    }

    async fn active_task(&self) -> eyre::Result<Option<Arc<dyn TaskOperations>>> {
        let active_task_id = self
            .store
            .inner
            .schedules
            .lock()
            .get(&self.schedule_id)
            .unwrap()
            .active_task;

        Ok(active_task_id.map(|task_id| {
            Arc::new(MemoryStoreTaskOperations {
                task_id,
                store: self.store.clone(),
            }) as Arc<_>
        }))
    }

    async fn cancel(&self) -> eyre::Result<()> {
        self.store.cancel_schedule(self.schedule_id)?;
        Ok(())
    }
}

fn filter_tasks(options: &Tasks) -> impl Fn(&&TaskState) -> bool + '_ {
    |task| {
        if let Some(statuses) = &options.include_status {
            if !statuses.contains(&task.status) {
                return false;
            }
        }

        if let Some(schedule_id) = options.schedule_id {
            if task.schedule_id != Some(schedule_id) {
                return false;
            }
        }

        if let Some(kind) = &options.kind {
            if task.definition.worker_selector.kind != kind.as_str() {
                return false;
            }
        }

        if let Some(added_after) = options.added_after {
            if task.added_at < added_after.into() {
                return false;
            }
        }
        if let Some(added_before) = options.added_before {
            if task.added_at >= added_before.into() {
                return false;
            }
        }
        if let Some(finished_after) = options.finished_after {
            if let Some(succeeded_at) = task.succeeded_at {
                if succeeded_at < finished_after.into() {
                    return false;
                }
            } else if let Some(failed_at) = task.failed_at {
                if failed_at < finished_after.into() {
                    return false;
                }
            } else if let Some(cancelled_at) = task.cancelled_at {
                if cancelled_at < finished_after.into() {
                    return false;
                }
            } else {
                return false;
            }
        }
        if let Some(finished_before) = options.finished_before {
            if let Some(succeeded_at) = task.succeeded_at {
                if succeeded_at >= finished_before.into() {
                    return false;
                }
            } else if let Some(failed_at) = task.failed_at {
                if failed_at >= finished_before.into() {
                    return false;
                }
            } else if let Some(cancelled_at) = task.cancelled_at {
                if cancelled_at >= finished_before.into() {
                    return false;
                }
            } else {
                return false;
            }
        }
        if let Some(target_after) = options.target_after {
            if task.definition.target < target_after.into() {
                return false;
            }
        }
        if let Some(target_before) = options.target_before {
            if task.definition.target >= target_before.into() {
                return false;
            }
        }

        if let Some(labels) = &options.include_labels {
            for label in labels {
                let task_label_value = match task.definition.labels.get(&label.label) {
                    Some(v) => v,
                    None => return false,
                };

                match &label.value {
                    LabelValueMatch::Value(expected_value) => {
                        if task_label_value != expected_value {
                            return false;
                        }
                    }
                    LabelValueMatch::AnyValue => {}
                }
            }
        }

        if let Some(search) = &options.search {
            if TwoWaySearcher::new(search.as_bytes())
                .search_in(&task.definition.data)
                .is_none()
            {
                return false;
            };
        }

        true
    }
}

fn filter_schedules(options: &Schedules) -> impl Fn(&&ScheduleState) -> bool + '_ {
    |schedule| {
        if let Some(active) = options.active {
            if schedule.active != active {
                return false;
            }
        }

        if let Some(kind) = &options.kind {
            match &schedule.definition.new_task {
                ora_common::schedule::NewTask::Repeat { task } => {
                    if task.worker_selector.kind != kind.as_str() {
                        return false;
                    }
                }
            }
        }

        if let Some(added_after) = options.added_after {
            if schedule.added_at < added_after.into() {
                return false;
            }
        }
        if let Some(added_before) = options.added_before {
            if schedule.added_at >= added_before.into() {
                return false;
            }
        }
        if let Some(cancelled_after) = options.cancelled_after {
            if let Some(succeeded_at) = schedule.cancelled_at {
                if succeeded_at < cancelled_after.into() {
                    return false;
                }
            } else {
                return false;
            }
        }
        if let Some(cancelled_before) = options.cancelled_before {
            if let Some(succeeded_at) = schedule.cancelled_at {
                if succeeded_at >= cancelled_before.into() {
                    return false;
                }
            } else {
                return false;
            }
        }

        if let Some(labels) = &options.include_labels {
            for label in labels {
                let task_label_value = match schedule.definition.labels.get(&label.label) {
                    Some(v) => v,
                    None => return false,
                };

                match &label.value {
                    LabelValueMatch::Value(expected_value) => {
                        if task_label_value != expected_value {
                            return false;
                        }
                    }
                    LabelValueMatch::AnyValue => {}
                }
            }
        }

        true
    }
}
