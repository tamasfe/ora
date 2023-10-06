use ora::Task;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

#[derive(Serialize, Deserialize)]
pub struct LatencyTestTask {
    pub target: OffsetDateTime,
}

impl Task for LatencyTestTask {
    type Output = Duration;
}

#[derive(Serialize, Deserialize)]
pub struct ScheduleTestTask;

impl Task for ScheduleTestTask {
    type Output = ();
}

#[derive(Serialize, Deserialize)]
pub struct ScheduleCancellationTestTask;

impl Task for ScheduleCancellationTestTask {
    type Output = ();
}
