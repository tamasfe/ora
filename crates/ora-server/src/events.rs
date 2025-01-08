//! Various events within the server, this can be used
//! for auditing purposes or triggering operations.

use paste::paste;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An event that occurred within the server.
///
/// These events encompass a wide range of actions
/// and can be used for many purposes, such as
/// auditing, triggering operations, or monitoring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// The timestamp of the event.
    pub timestamp: std::time::SystemTime,
    /// The event that occurred.
    pub kind: AuditEventKind,
}

/// The kind of event that occurred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventKind {
    /// A job was created.
    JobAdded {
        /// The ID of the job that was added.
        job_id: Uuid,
        /// The type of the job that was added.
        job_type_id: String,
    },
    /// A job was cancelled.
    JobCancelled {
        /// The ID of the job that was added.
        job_id: Uuid,
    },
    /// A job was deleted.
    JobDeleted {
        /// The ID of the job that was deleted.
        job_id: Uuid,
    },
    /// A new execution was scheduled for a job.
    ExecutionAdded {
        /// The ID of the execution.
        execution_id: Uuid,
        /// The ID of the job the execution
        /// is associated with.
        job_id: Uuid,
        /// The target execution time.
        target_execution_time: std::time::SystemTime,
    },
    /// An execution is ready to be
    /// executed.
    ExecutionReady {
        /// The ID of the execution.
        execution_id: Uuid,
    },
    /// An execution was assigned to an executor.
    ExecutionAssigned {
        /// The ID of the execution.
        execution_id: Uuid,
        /// The ID of the executor.
        executor_id: Uuid,
    },
    /// An execution was started by an executor.
    ExecutionStarted {
        /// The ID of the execution.
        execution_id: Uuid,
    },
    /// An execution has succeeded.
    ExecutionSucceeded {
        /// The ID of the execution.
        execution_id: Uuid,
    },
    /// An execution has failed.
    ExecutionFailed {
        /// The ID of the execution.
        execution_id: Uuid,
        /// Whether there will be no more
        /// attempts to execute the job.
        terminal: bool,
    },
    /// An executor has connected.
    ExecutorConnected {
        /// The ID of the executor.
        executor_id: Uuid,
    },
    /// An executor has disconnected.
    ExecutorDisconnected {
        /// The ID of the executor.
        executor_id: Uuid,
    },
    /// A schedule was created.
    ScheduleAdded {
        /// The ID of the schedule.
        schedule_id: Uuid,
    },
    /// A schedule was cancelled.
    ScheduleCancelled {
        /// The ID of the schedule.
        schedule_id: Uuid,
    },
    /// A schedule was marked as unschedulable and no more
    /// jobs will be created from it.
    ScheduleUnschedulable {
        /// The ID of the schedule.
        schedule_id: Uuid,
    },
    /// Schedule was deleted.
    ScheduleDeleted {
        /// The ID of the schedule.
        schedule_id: Uuid,
    },
    /// A snapshot was exported.
    SnapshotExported,
    /// A snapshot was imported.
    SnapshotImported,
}

/// Events associated with executions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionEvent {
    /// New executions are ready.
    TimedExecutionsReady,
    /// New executions are ready to run.
    ExecutionsReadyToRun,
    /// Executions were added.
    ExecutionsAdded,
    /// Executions have started.
    ExecutionsFinished,
}

/// Events associated with jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobEvent {
    /// New jobs were added.
    JobsCreated,
}

/// Events associated with schedules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScheduleEvent {
    /// New schedules were added.
    SchedulesAdded,
}

/// Events associated with executors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutorEvent {
    /// An executor has been added.
    ExecutorReady,
}

macro_rules! create_bus {
    ($($event_name:ident),*$(,)?) => {
        paste! {
            /// A bus for various events
            #[derive(Debug, Clone)]
            pub(crate) struct EventBus {
                $(
                    [<$event_name:snake s>]: tokio::sync::broadcast::Sender<[<$event_name>]>,
                )*
                audit_events: tokio::sync::broadcast::Sender<AuditEvent>,

            }

            impl EventBus {
                /// Create a new event bus with the given capacity.
                pub fn new(capacity: usize) -> Self {
                    Self {
                        $(
                            [<$event_name:snake s>]: tokio::sync::broadcast::Sender::new(capacity),
                        )*
                        audit_events: tokio::sync::broadcast::Sender::new(capacity),
                    }
                }

                $(
                    /// Emit the given event.
                    pub(crate) fn [<emit_ $event_name:snake>](&self, event: [<$event_name>]) {
                        _ = self.[<$event_name:snake s>].send(event);
                    }

                    /// Subscribe to the given event.
                    pub(crate) fn [<subscribe_ $event_name:snake s>](&self) -> impl futures::Stream<Item = [<$event_name>]> {
                        use futures::StreamExt;

                        tokio_stream::wrappers::BroadcastStream::new(self.[<$event_name:snake s>].subscribe())
                            .filter_map(|event| {
                                if event.is_err() {
                                    tracing::warn!("subscription has lagged behind, some events have been dropped");
                                }

                                futures::future::ready(event.ok())
                            })
                    }
                )*

                pub(crate) fn emit_audit_event(&self, create_event: impl FnOnce() -> AuditEventKind) {
                    _ = self.audit_events.send(AuditEvent {
                        timestamp: std::time::SystemTime::now(),
                        kind: create_event(),
                    });
                }

                pub(crate) fn audit_events_enabled(&self) -> bool {
                    self.audit_events.receiver_count() > 0
                }

                pub(crate) fn subscribe_audit_events(&self) -> impl futures::Stream<Item = AuditEvent> + Unpin + Send + Sync + 'static {
                    use futures::StreamExt;

                    tokio_stream::wrappers::BroadcastStream::new(self.audit_events.subscribe())
                        .filter_map(|event| {
                            if event.is_err() {
                                tracing::warn!("subscription has lagged behind, some events have been dropped");
                            }

                            futures::future::ready(event.ok())
                        })
                }
            }
        }
    };
}

create_bus!(ExecutionEvent, JobEvent, ExecutorEvent, ScheduleEvent);
