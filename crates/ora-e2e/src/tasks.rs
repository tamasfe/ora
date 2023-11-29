use ora::{DeriveTaskExtra, Task};
use schemars::JsonSchema;
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

/// A task with derived implementation.
#[derive(Task, Serialize, Deserialize, JsonSchema)]
pub struct FooTask {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct BarOutput;

/// A task with derived implementation.
#[derive(Task, Serialize, Deserialize, JsonSchema)]
#[task(output = "BarOutput")]
pub struct BarTask {}

/// A task with derived implementation and additional
/// settings.
#[derive(Task, Serialize, Deserialize, JsonSchema)]
#[task(extra)]
pub struct BazTask {}

impl DeriveTaskExtra for BazTask {
    fn metadata(inferred: ora::TaskMetadata) -> ora::TaskMetadata {
        inferred
    }
}

#[derive(Task, Serialize, Deserialize)]
#[task(no_schema, output = "usize")]
pub struct NoSchemaTask {}

#[derive(Task, Serialize, Deserialize, JsonSchema)]
#[task(timeout = "30s")]
pub struct TimeoutTask {}
