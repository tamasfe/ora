//! A collection of jobs and their handlers.

use std::time::SystemTime;

use eyre::bail;
use futures::future::pending;
use ora::executor::{self, ExecutionContext};

use ora::JobType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A job that calculates the length of a string.
#[derive(JobType, Serialize, Deserialize, JsonSchema)]
#[ora(output = "usize")]
pub struct StrLen {
    /// The string to calculate the length of.
    pub string: String,
}

/// The handler for the [`StrLen`] job.
pub async fn str_len_handler(_context: ExecutionContext, job: StrLen) -> executor::Result<usize> {
    Ok(job.string.len())
}

/// A job that calculates the sum of two numbers.
#[derive(JobType, Serialize, Deserialize, JsonSchema)]
#[ora(output = "i32")]
pub struct Add {
    /// The first number.
    pub a: i32,
    /// The second number.
    pub b: i32,
}

/// The handler for the [`Add`] job.
pub async fn add_handler(_context: ExecutionContext, job: Add) -> executor::Result<i32> {
    Ok(job.a + job.b)
}

/// A job that times out after 2 seconds by default.
#[derive(JobType, Serialize, Deserialize, JsonSchema)]
#[ora(timeout = "2s")]
pub struct Timeout {
    /// The number of seconds to sleep.
    pub seconds: u64,
}

/// The handler for the [`Timeout`] job.
pub async fn timeout_handler(_context: ExecutionContext, job: Timeout) -> executor::Result<()> {
    tokio::time::sleep(std::time::Duration::from_secs(job.seconds)).await;
    Ok(())
}

/// A job that is retried 3 times.
#[derive(JobType, Serialize, Deserialize, JsonSchema)]
#[ora(retries = 3)]
pub struct Retry3 {
    pub succeed_at_attempt: u64,
}

/// The handler for the [`Retry3`] job.
pub async fn retry3_handler(context: ExecutionContext, job: Retry3) -> executor::Result<()> {
    if context.attempt_number() < job.succeed_at_attempt {
        bail!("This job fails on purpose");
    }

    Ok(())
}

/// A job that panics.
#[derive(JobType, Serialize, Deserialize, JsonSchema)]
pub struct Panic;

/// The handler for the [`Panic`] job.
pub async fn panic_handler(_context: ExecutionContext, _job: Panic) -> executor::Result<()> {
    panic!("This job panics on purpose");
}

/// A job that waits forever and is retried 3 times.
#[derive(JobType, Serialize, Deserialize, JsonSchema)]
#[ora(retries = 3)]
pub struct WaitForever;

/// The handler for the [`WaitForever`] job.
pub async fn wait_forever_handler(
    _context: ExecutionContext,
    _job: WaitForever,
) -> executor::Result<()> {
    pending().await
}

/// A long running job that can be retried many times.
#[derive(JobType, Serialize, Deserialize, JsonSchema)]
#[ora(retries = u64::MAX)]
pub struct LongRunning;

/// A job that returns whether the target
/// execution time is within the specified range.
#[derive(JobType, Serialize, Deserialize, JsonSchema)]
pub struct AssertExecutionTime {
    pub after: SystemTime,
    pub before: SystemTime,
}

/// The handler for the [`AssertExecutionTime`] job.
pub async fn assert_execution_time_handler(
    context: ExecutionContext,
    job: AssertExecutionTime,
) -> executor::Result<()> {
    let target = context.target_execution_time();
    assert!(target >= job.after);
    assert!(target < job.before);
    Ok(())
}
