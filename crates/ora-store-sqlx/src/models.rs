#![allow(unused, dead_code)]
use std::sync::Arc;

use ora_scheduler::store::{schedule::SchedulerScheduleStoreEvent, task::SchedulerTaskStoreEvent};
use ora_worker::store::WorkerStoreEvent;
use sea_query::Iden;
use uuid::Uuid;

/// Identifier for the ora schema.
#[derive(Iden)]
pub(crate) struct Ora;

/// Task table column identifiers.
#[derive(Iden)]
pub(crate) enum Task {
    Table,
    Id,
    Status,
    Active,
    ScheduleId,
    // region: definition fields
    Target,
    WorkerSelector,
    DataBytes,
    DataJson,
    DataFormat,
    Labels,
    TimeoutPolicy,
    // endregion
    // region: events timestamps
    AddedAt,
    ReadyAt,
    StartedAt,
    SucceededAt,
    FailedAt,
    CancelledAt,
    // endregion
    // region: outcome
    OutputBytes,
    OutputJson,
    OutputFormat,
    FailureReason,
    // endregion
    // region: worker
    WorkerId,
    // endregion
}

/// Schedule table column identifiers.
#[derive(Iden)]
pub(crate) enum Schedule {
    Table,
    Id,
    Active,
    ActiveTaskId,

    // region: definition fields
    SchedulePolicy,
    Immediate,
    MissedTasksPolicy,
    NewTask,
    Labels,
    // endregion

    // region: events timestamps
    AddedAt,
    CancelledAt,
    // endregion
}

// A union of all essential events emitted inside the `DbStore`.
#[derive(Debug, Clone)]
pub(crate) enum DbEvent {
    SchedulerScheduleStore(SchedulerScheduleStoreEvent),
    SchedulerTaskStore(SchedulerTaskStoreEvent),
    WorkerStore(WorkerStoreEvent),
    /// A task has succeeded, failed or was cancelled.
    TaskDone(Uuid),
}
