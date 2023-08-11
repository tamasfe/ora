//! Backend store types for managing schedules.

use async_trait::async_trait;
use futures::Stream;
use ora_common::{schedule::ScheduleDefinition, task::TaskDefinition, UnixNanos};
use uuid::Uuid;

/// A store for managing schedules.
#[async_trait]
pub trait SchedulerScheduleStore {
    /// An error type returned by operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// An event stream that can be used to watch for changes.
    type Events: Stream<Item = Result<SchedulerScheduleStoreEvent, Self::Error>>;

    /// Subscribe for new events.
    async fn events(&self) -> Result<Self::Events, Self::Error>;

    /// Return all schedules that have no associated active tasks.
    async fn pending_schedules(&self) -> Result<Vec<ActiveSchedule>, Self::Error>;

    /// Return the schedule of a task only if the schedule
    /// has no associated active tasks.
    async fn pending_schedule_of_task(
        &self,
        task_id: Uuid,
    ) -> Result<Option<ActiveSchedule>, Self::Error>;

    /// Return the target time of a task.
    async fn task_target(&self, task_id: Uuid) -> Result<UnixNanos, Self::Error>;

    /// Add a new active task for a schedule.
    async fn add_task(&self, schedule_id: Uuid, task: TaskDefinition) -> Result<(), Self::Error>;
}

/// A currently active schedule.
#[derive(Debug, Clone)]
pub struct ActiveSchedule {
    /// The schedule's ID.
    pub id: Uuid,
    /// The schedule's defintion that is used for
    /// spawning new tasks.
    pub definition: ScheduleDefinition,
}

/// A scheduler event.
#[derive(Debug, Clone)]
pub enum SchedulerScheduleStoreEvent {
    /// A new task was added.
    ScheduleAdded(Box<ActiveSchedule>),
    /// A task was finished for any reason.
    TaskFinished(Uuid),
    /// A schedule was cancelled.
    ScheduleCancelled(Uuid),
}
