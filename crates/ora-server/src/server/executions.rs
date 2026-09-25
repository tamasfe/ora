//! The server implementation for Ora.

use std::{
    collections::HashSet,
    pin::pin,
    sync::Arc,
    time::{Duration, SystemTime},
};

use ora_backend::{
    Backend,
    executions::{FailedExecution, RetriedExecution, StartedExecution, SucceededExecution},
    jobs::{RetryPolicy, TimeoutBaseTime},
};
use tokio::time::Instant;
use wgroup::WaitGuard;

use crate::{
    executor_pool::{ExecutorEvent, ExecutorPool},
    util::{deadline_after, validate_json},
};

use futures::StreamExt;

#[tracing::instrument(skip_all)]
pub(super) async fn ready_executions_loop(
    backend: Arc<impl Backend>,
    executor_pool: ExecutorPool,
    wg: WaitGuard,
) {
    'main_loop: loop {
        // Created before assigning executions so that
        // capacity freed up in the meantime is not missed.
        let executor_available = executor_pool.executor_available();

        // Executions that are offered (or accepted but not yet started)
        // are still pending in the backend, they are not fetched again.
        let in_flight_execution_ids = executor_pool.in_flight_execution_ids();

        let mut stream = pin!(backend.ready_executions(&in_flight_execution_ids));

        // We keep track of executions we could not offer
        // (or were already offered) otherwise `wait_for_ready_executions`
        // would keep immediately returning as they are still ready,
        // causing a busy loop.
        let mut ignored_execution_ids = Vec::new();

        // Taken before each batch is fetched, so that the executor pool
        // can detect executions that were started since.
        let mut fetched_at = std::time::Instant::now();

        // Whether the executors cannot accept any more executions,
        // in which case the remaining ready executions are not fetched.
        let mut saturated = false;

        while let Some(ready_executions) = stream.next().await {
            if wg.is_waiting() {
                tracing::debug!("shutting down");
                break 'main_loop;
            }

            let ready_executions = match ready_executions {
                Ok(ready_executions) => ready_executions,
                Err(error) => {
                    tracing::error!(%error, "error fetching ready executions");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue 'main_loop;
                }
            };

            let ready_count = ready_executions.len();

            let offered = executor_pool.try_offer(ready_executions, fetched_at);
            ignored_execution_ids.extend(offered.not_offered);

            tracing::debug!(
                %ready_count,
                offered_count = offered.offered_count,
                "offered ready executions to executors"
            );

            if !executor_pool.has_capacity() {
                saturated = true;
                break;
            }

            fetched_at = std::time::Instant::now();
        }

        // Executions offered during this pass are still pending as well.
        ignored_execution_ids.extend(executor_pool.in_flight_execution_ids());

        let mut check_deadline = Instant::now() + std::time::Duration::from_secs(5);

        // Executions might not have been offered only because executors
        // rejected some earlier, in which case we can check again
        // once they can be offered executions again.
        if let Some(backoff_end) = executor_pool.earliest_backoff_end() {
            check_deadline = check_deadline.min(Instant::from_std(backoff_end));
        }

        let check_delay = tokio::time::sleep_until(check_deadline);

        if saturated {
            // Nothing can be assigned until executors free up capacity,
            // so there is no point in waiting for ready executions.
            tokio::select! {
                _ = check_delay => {
                    tracing::trace!("periodic check for ready executions");
                },
                () = executor_available => {},
                _ = wg.waiting() => {
                    tracing::debug!("shutting down");
                    break 'main_loop;
                }
            }
        } else {
            tokio::select! {
                _ = check_delay => {
                    tracing::trace!("periodic check for ready executions");
                },
                _ = backend.wait_for_ready_executions(&ignored_execution_ids) => {},
                // Executions that were not offered might be offered now.
                () = executor_available, if !ignored_execution_ids.is_empty() => {},
                _ = wg.waiting() => {
                    tracing::debug!("shutting down");
                    break 'main_loop;
                }
            }
        }
    }
}

/// The maximum number of executor events processed together.
const MAX_EXECUTOR_EVENT_BATCH: usize = 1000;

/// The maximum number of accepted executions started together.
const MAX_START_BATCH: usize = 1000;

/// The maximum time execution results wait for
/// the executions to be started in the backend.
const START_WAIT_TIMEOUT: Duration = Duration::from_mins(1);

/// Start executions accepted by executors in the backend.
///
/// This is separate from processing the other executor events,
/// so that starting executions is not delayed by processing results.
#[tracing::instrument(skip_all)]
pub(super) async fn execution_starts_loop(
    backend: Arc<impl Backend>,
    executor_pool: ExecutorPool,
    accepted_executions: flume::Receiver<StartedExecution>,
    wg: WaitGuard,
) {
    // Executions accepted before shutdown are still started,
    // their results are processed until all executors are gone.
    let mut shutdown_deadline: Option<Instant> = None;

    loop {
        let execution = if let Some(shutdown_deadline) = shutdown_deadline {
            if executor_pool.is_empty() && accepted_executions.is_empty() {
                tracing::debug!("all executors disconnected, shutting down execution starts loop");
                return;
            }

            tokio::select! {
                () = tokio::time::sleep_until(shutdown_deadline) => {
                    tracing::warn!(
                        execution_count = accepted_executions.len(),
                        "shutdown deadline elapsed, not starting remaining accepted executions"
                    );
                    return;
                }
                // Check whether all executors disconnected.
                () = tokio::time::sleep(Duration::from_millis(100)) => continue,
                execution = accepted_executions.recv_async() => {
                    match execution {
                        Ok(execution) => execution,
                        Err(_) => {
                            tracing::debug!("accepted executions channel closed, shutting down execution starts loop");
                            return;
                        }
                    }
                },
            }
        } else {
            tokio::select! {
                () = wg.waiting() => {
                    tracing::debug!("waiting for executors before shutting down execution starts loop");
                    shutdown_deadline = Some(shutdown_events_deadline(&executor_pool));
                    continue;
                },
                execution = accepted_executions.recv_async() => {
                    match execution {
                        Ok(execution) => execution,
                        Err(_) => {
                            tracing::debug!("accepted executions channel closed, shutting down execution starts loop");
                            return;
                        }
                    }
                },
            }
        };

        // Executions accepted while the previous batch was started
        // are started together.
        let started_executions = std::iter::once(execution)
            .chain(accepted_executions.try_iter().take(MAX_START_BATCH - 1))
            .collect::<Vec<_>>();

        start_executions(&*backend, &executor_pool, &wg, started_executions).await;
    }
}

/// Additional time given to executors on top of the shutdown grace period
/// before the executor events are not processed anymore.
const SHUTDOWN_EVENTS_MARGIN: Duration = Duration::from_secs(5);

/// The time after which executor events are not processed anymore on shutdown.
fn shutdown_events_deadline(executor_pool: &ExecutorPool) -> Instant {
    Instant::from_std(deadline_after(
        executor_pool
            .shutdown_grace_period()
            .saturating_add(SHUTDOWN_EVENTS_MARGIN),
    ))
}

#[tracing::instrument(skip_all)]
pub(super) async fn executor_events_loop(
    backend: Arc<impl Backend>,
    executor_pool: ExecutorPool,
    executor_events: flume::Receiver<ExecutorEvent>,
    wg: WaitGuard,
) {
    // Executors are given a grace period on shutdown to finish their
    // executions, the events are processed until all of them are gone.
    let mut shutdown_deadline: Option<Instant> = None;

    loop {
        let event = if let Some(shutdown_deadline) = shutdown_deadline {
            if executor_pool.is_empty() && executor_events.is_empty() {
                tracing::debug!("all executors disconnected, shutting down executor events loop");
                return;
            }

            tokio::select! {
                () = tokio::time::sleep_until(shutdown_deadline) => {
                    tracing::warn!(
                        event_count = executor_events.len(),
                        "shutdown deadline elapsed, dropping remaining executor events"
                    );
                    return;
                }
                // Check whether all executors disconnected.
                () = tokio::time::sleep(Duration::from_millis(100)) => continue,
                event = executor_events.recv_async() => {
                    match event {
                        Ok(event) => event,
                        Err(_) => {
                            tracing::debug!("executor events channel closed, shutting down executor events loop");
                            return;
                        }
                    }
                },
            }
        } else {
            tokio::select! {
                () = wg.waiting() => {
                    tracing::debug!("waiting for executors before shutting down executor events loop");
                    shutdown_deadline = Some(shutdown_events_deadline(&executor_pool));
                    continue;
                },
                event = executor_events.recv_async() => {
                    match event {
                        Ok(event) => event,
                        Err(_) => {
                            tracing::debug!("executor events channel closed, shutting down executor events loop");
                            return;
                        }
                    }
                },
            }
        };

        // Events that arrived while the previous batch was processed
        // are processed together, so that the backend can
        // update them in a single operation.
        let events = std::iter::once(event).chain(
            executor_events
                .try_iter()
                .take(MAX_EXECUTOR_EVENT_BATCH - 1),
        );

        let mut succeeded_executions = Vec::new();
        let mut maybe_retry_executions = Vec::new();

        for event in events {
            match event {
                ExecutorEvent::ExecutionSucceeded {
                    job_id,
                    execution_id,
                    timestamp,
                    output_payload_json,
                    retry_policy,
                    attempt_number,
                } => {
                    if let Err(error) = validate_json(&output_payload_json) {
                        maybe_retry_executions.push(MaybeRetryExecution {
                            execution: FailedExecution {
                                job_id,
                                execution_id,
                                failed_at: timestamp,
                                failure_reason: format!(
                                    "invalid output returned by the executor: {error}"
                                ),
                            },
                            retry_policy,
                            attempt_number,
                        });

                        continue;
                    }

                    succeeded_executions.push(SucceededExecution {
                        execution_id,
                        succeeded_at: timestamp,
                        output_json: output_payload_json,
                    });
                }
                ExecutorEvent::ExecutionFailed {
                    job_id,
                    execution_id,
                    timestamp,
                    failure_reason,
                    retry_policy,
                    attempt_number,
                } => {
                    maybe_retry_executions.push(MaybeRetryExecution {
                        execution: FailedExecution {
                            job_id,
                            execution_id,
                            failed_at: timestamp,
                            failure_reason,
                        },
                        retry_policy,
                        attempt_number,
                    });
                }
                ExecutorEvent::ExecutorDisconnected { executor } => {
                    maybe_retry_executions.extend(executor.assigned_executions().into_iter().map(
                        |execution| MaybeRetryExecution {
                            execution: FailedExecution {
                                job_id: execution.job_id,
                                execution_id: execution.execution_id,
                                failed_at: SystemTime::now(),
                                failure_reason: "executor disconnected".to_string(),
                            },
                            retry_policy: execution.retry_policy,
                            attempt_number: execution.attempt_number,
                        },
                    ));
                }
                ExecutorEvent::JobTypesAdded { job_types } => {
                    if let Err(error) = backend.add_job_types(&job_types).await {
                        tracing::error!(%error, "error adding job types to backend");
                    }
                }
            }
        }

        // Executions must be started in the backend before they can succeed or fail,
        // they were accepted before their results were received.
        let result_execution_ids = succeeded_executions
            .iter()
            .map(|execution| execution.execution_id)
            .chain(
                maybe_retry_executions
                    .iter()
                    .map(|execution| execution.execution.execution_id),
            )
            .collect::<Vec<_>>();

        if !result_execution_ids.is_empty()
            && !executor_pool
                .wait_for_starts(&result_execution_ids, START_WAIT_TIMEOUT)
                .await
        {
            tracing::warn!(
                "timed out waiting for executions to be started before processing their results"
            );
        }

        if !succeeded_executions.is_empty()
            && let Err(error) =
                retry_backend(&wg, "updating succeeded executions", u32::MAX, || {
                    backend.executions_succeeded(&succeeded_executions)
                })
                .await
        {
            tracing::error!(%error, "error updating succeeded executions");
        }

        if !maybe_retry_executions.is_empty() {
            retry_executions(&*backend, &wg, maybe_retry_executions).await;
        }
    }
}

/// Start executions accepted by executors in the backend.
///
/// Executions that cannot be started are cancelled on the executors.
async fn start_executions(
    backend: &impl Backend,
    executor_pool: &ExecutorPool,
    wg: &WaitGuard,
    started_executions: Vec<StartedExecution>,
) {
    let execution_ids = started_executions
        .iter()
        .map(|execution| execution.execution_id)
        .collect::<Vec<_>>();

    // Starting is retried until it succeeds (only a few times during shutdown),
    // giving up could leave executions in progress in the backend
    // if an attempt that seemingly failed was committed, while
    // the executors were told to cancel them. Repeated calls
    // are fine, as executions already started by the same executor
    // are returned again.
    match retry_backend(wg, "updating started executions", u32::MAX, || {
        backend.executions_started(&started_executions)
    })
    .await
    {
        Ok(started_execution_ids) => {
            executor_pool.accepted_executions_started(&execution_ids);

            // Executions that are not pending anymore (e.g. cancelled
            // since they were fetched) must not run.
            if started_execution_ids.len() != execution_ids.len() {
                let started_execution_ids =
                    started_execution_ids.into_iter().collect::<HashSet<_>>();

                let not_started_execution_ids = execution_ids
                    .into_iter()
                    .filter(|id| !started_execution_ids.contains(id))
                    .collect::<Vec<_>>();

                tracing::debug!(
                    execution_count = not_started_execution_ids.len(),
                    "accepted executions are not pending anymore, cancelling them"
                );

                executor_pool.cancel_execution_ids(&not_started_execution_ids);
            }
        }
        Err(error) => {
            tracing::error!(%error, "error updating started executions");

            // This only happens during shutdown, the executions are most likely
            // still pending in the backend and will be offered again. Otherwise they
            // are found by the next server as orphaned in-progress executions.
            executor_pool.cancel_execution_ids(&execution_ids);
            executor_pool.accepted_executions_not_started(&execution_ids);
        }
    }
}

#[tracing::instrument(skip_all)]
pub(super) async fn execution_timeouts_loop(
    backend: Arc<impl Backend>,
    executor_pool: ExecutorPool,
    wg: WaitGuard,
) {
    'main_loop: loop {
        let mut stream = pin!(backend.in_progress_executions());

        while let Some(in_progress_executions) = stream.next().await {
            let in_progress_executions = match in_progress_executions {
                Ok(in_progress_executions) => in_progress_executions,
                Err(error) => {
                    tracing::error!(%error, "error fetching in-progress executions");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue 'main_loop;
                }
            };

            let now = SystemTime::now();
            let mut timed_out_executions = Vec::new();
            let mut orphaned_executions = Vec::new();

            for execution in in_progress_executions {
                if !executor_pool.executor_exists(&execution.executor_id) {
                    orphaned_executions.push(MaybeRetryExecution {
                        execution: FailedExecution {
                            job_id: execution.job_id,
                            execution_id: execution.execution_id,
                            failed_at: now,
                            failure_reason: "executor disconnected".to_string(),
                        },
                        retry_policy: execution.retry_policy,
                        attempt_number: execution.attempt_number,
                    });
                    continue;
                }

                if execution.timeout_policy.timeout.is_zero() {
                    continue;
                }

                let base_time = match execution.timeout_policy.base_time {
                    TimeoutBaseTime::TargetExecutionTime => execution.target_execution_time,
                    TimeoutBaseTime::StartTime => execution.started_at,
                };

                // A deadline that is not representable is never reached.
                let Some(deadline) = base_time.checked_add(execution.timeout_policy.timeout) else {
                    continue;
                };

                if now > deadline {
                    timed_out_executions.push(MaybeRetryExecution {
                        execution: FailedExecution {
                            job_id: execution.job_id,
                            execution_id: execution.execution_id,
                            failed_at: now,
                            failure_reason: "execution timed out".to_string(),
                        },
                        retry_policy: execution.retry_policy,
                        attempt_number: execution.attempt_number,
                    });
                }
            }

            if !timed_out_executions.is_empty() {
                let execution_count = timed_out_executions.len();
                tracing::info!(execution_count, "executions timed out");

                let timed_out_execution_ids = timed_out_executions
                    .iter()
                    .map(|execution| execution.execution.execution_id)
                    .collect::<Vec<_>>();

                retry_executions(&*backend, &wg, timed_out_executions).await;

                // The executors might still be running the executions,
                // occupying capacity.
                executor_pool.cancel_execution_ids(&timed_out_execution_ids);
            }

            if !orphaned_executions.is_empty() {
                let execution_count = orphaned_executions.len();
                tracing::info!(execution_count, "orphaned executions found");

                retry_executions(&*backend, &wg, orphaned_executions).await;
            }
        }

        let check_delay = tokio::time::sleep(std::time::Duration::from_secs(1));

        tokio::select! {
            _ = wg.waiting() => {
                tracing::debug!("shutting down");
                break 'main_loop;
            },
            _ = check_delay => {
                tracing::trace!("periodic check for execution timeouts");
            }
        }
    }
}

pub(super) struct MaybeRetryExecution {
    pub(super) execution: FailedExecution,
    pub(super) retry_policy: RetryPolicy,
    pub(super) attempt_number: u64,
}

/// The maximum backoff duration of retries,
/// so that the retry times are always representable.
const MAX_RETRY_BACKOFF: Duration = Duration::from_hours(365 * 24);

async fn retry_executions<B>(backend: &B, wg: &WaitGuard, executions: Vec<MaybeRetryExecution>)
where
    B: Backend,
{
    let mut failed_executions = Vec::new();
    let mut retried_executions = Vec::new();

    for execution in executions {
        if execution.attempt_number <= execution.retry_policy.retries {
            let backoff_duration = retry_backoff(&execution.retry_policy, execution.attempt_number);

            tracing::info!(
                execution_id = %execution.execution.execution_id,
                attempt_number = execution.attempt_number,
                backoff_duration = ?backoff_duration,
                "retrying execution"
            );

            retried_executions.push(RetriedExecution {
                failed_execution: execution.execution,
                retry_execution_time: SystemTime::now() + backoff_duration,
            });
        } else {
            tracing::debug!(
                execution_id = %execution.execution.execution_id,
                attempt_number = execution.attempt_number,
                "execution reached max attempts, will not be retried"
            );

            failed_executions.push(execution.execution);
        }
    }

    if !failed_executions.is_empty() {
        if let Err(error) = retry_backend(wg, "updating failed executions", u32::MAX, || {
            backend.executions_failed(&failed_executions)
        })
        .await
        {
            tracing::error!(%error, "error updating failed executions");
        } else {
            tracing::debug!(
                count = failed_executions.len(),
                "executions reached max retry attempts"
            );
        }
    }

    if !retried_executions.is_empty()
        && let Err(error) = retry_backend(wg, "updating retried executions", u32::MAX, || {
            backend.executions_retried(&retried_executions)
        })
        .await
    {
        tracing::error!(%error, "error updating retried executions");
    }
}

/// The backoff duration before retrying after the given attempt.
fn retry_backoff(retry_policy: &RetryPolicy, attempt_number: u64) -> Duration {
    let backoff_duration = match retry_policy.backoff_strategy {
        ora_backend::jobs::BackoffStrategy::Fixed => retry_policy.backoff_duration,
        ora_backend::jobs::BackoffStrategy::Exponential => {
            let backoff_multiplier = u32::try_from(attempt_number.saturating_sub(1))
                .ok()
                .and_then(|exp| 2u32.checked_pow(exp))
                .unwrap_or(u32::MAX);

            retry_policy
                .backoff_duration
                .saturating_mul(backoff_multiplier)
        }
    };

    let backoff_duration = match retry_policy.max_backoff_duration {
        Some(max_backoff) => backoff_duration.min(max_backoff),
        None => backoff_duration,
    };

    backoff_duration.min(MAX_RETRY_BACKOFF)
}

/// The delay after backend errors before trying again.
const BACKEND_ERROR_DELAY: Duration = Duration::from_secs(5);

/// The maximum attempts of backend operations during shutdown.
const MAX_SHUTDOWN_ATTEMPTS: u32 = 3;

/// Run a backend operation until it succeeds, as errors might be transient.
///
/// Gives up after `max_attempts`, or fewer while shutting down.
async fn retry_backend<T, E, F>(
    wg: &WaitGuard,
    operation: &str,
    max_attempts: u32,
    mut f: impl FnMut() -> F,
) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt: u32 = 1;
    let mut delay = Duration::from_millis(100);

    loop {
        let error = match f().await {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };

        let max_attempts = if wg.is_waiting() {
            max_attempts.min(MAX_SHUTDOWN_ATTEMPTS)
        } else {
            max_attempts
        };

        if attempt >= max_attempts {
            return Err(error);
        }

        tracing::warn!(%error, attempt, "backend error while {operation}, retrying");

        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(BACKEND_ERROR_DELAY);
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use ora_backend::jobs::BackoffStrategy;

    use super::*;

    #[test]
    fn retry_backoff_saturates() {
        let policy = RetryPolicy {
            retries: u64::MAX,
            backoff_duration: Duration::from_secs(1),
            max_backoff_duration: None,
            backoff_strategy: BackoffStrategy::Exponential,
        };

        assert_eq!(retry_backoff(&policy, 1), Duration::from_secs(1));
        assert_eq!(retry_backoff(&policy, 3), Duration::from_secs(4));
        assert_eq!(retry_backoff(&policy, 1000), MAX_RETRY_BACKOFF);
        assert_eq!(retry_backoff(&policy, u64::MAX), MAX_RETRY_BACKOFF);

        let policy = RetryPolicy {
            max_backoff_duration: Some(Duration::from_secs(10)),
            ..policy
        };

        assert_eq!(retry_backoff(&policy, 1000), Duration::from_secs(10));

        let policy = RetryPolicy {
            backoff_duration: Duration::MAX,
            max_backoff_duration: None,
            backoff_strategy: BackoffStrategy::Fixed,
            ..policy
        };

        assert_eq!(retry_backoff(&policy, 1), MAX_RETRY_BACKOFF);
    }
}
