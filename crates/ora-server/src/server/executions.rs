//! The server implementation for Ora.

use std::{pin::pin, sync::Arc, time::SystemTime};

use ora_backend::{
    Backend,
    executions::{FailedExecution, RetriedExecution, SucceededExecution},
    jobs::{RetryPolicy, TimeoutBaseTime},
};
use wgroup::WaitGuard;

use crate::{
    executor_pool::{ExecutorEvent, ExecutorPool},
    util::validate_json,
};

use futures::StreamExt;

#[tracing::instrument(skip_all)]
pub(super) async fn ready_executions_loop(
    backend: Arc<impl Backend>,
    executor_pool: ExecutorPool,
    wg: WaitGuard,
) {
    'main_loop: loop {
        let mut stream = pin!(backend.ready_executions());

        // We keep track of executions we could not schedule
        // otherwise `wait_for_ready_executions` would
        // keep immediately returning as they are still ready,
        // causing a busy loop.
        let mut unassigned_execution_ids = Vec::new();

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

            let (assigned_executions, unassigned_executions) =
                executor_pool.try_assign(ready_executions);
            let assigned_count = assigned_executions.len();

            unassigned_execution_ids.extend(
                unassigned_executions
                    .into_iter()
                    .map(|execution| execution.execution_id),
            );

            tracing::debug!(
                %ready_count,
                %assigned_count,
                "assigned ready executions to executors"
            );

            if let Err(error) = backend.executions_started(&assigned_executions).await {
                tracing::error!(%error, "error updating assigned executions");
                // There's not a lot we can do here,
                // retries should be handled
                // by the backend implementation.
                //
                // In this scenario the given executions might be
                // assigned multiple times.
                //
                // We'll skip any remaining batches and recreate the stream.
                continue 'main_loop;
            }
        }

        let check_delay = tokio::time::sleep(std::time::Duration::from_secs(5));

        tokio::select! {
            _ = check_delay => {
                tracing::trace!("periodic check for ready executions");
            },
            _ = backend.wait_for_ready_executions(&unassigned_execution_ids) => {},
            _ = wg.waiting() => {
                tracing::debug!("shutting down");
                break 'main_loop;
            }
        }
    }
}

#[tracing::instrument(skip_all)]
pub(super) async fn executor_events_loop(
    backend: Arc<impl Backend>,
    executor_events: flume::Receiver<ExecutorEvent>,
    wg: WaitGuard,
) {
    loop {
        let event = tokio::select! {
            _ = wg.waiting() => {
                tracing::debug!("shutting down executor events loop");
                return;
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
        };

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
                    retry_executions(
                        &*backend,
                        vec![MaybeRetryExecution {
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
                        }],
                    )
                    .await;

                    continue;
                }

                if let Err(error) = backend
                    .executions_succeeded(&[SucceededExecution {
                        execution_id,
                        succeeded_at: timestamp,
                        output_json: output_payload_json,
                    }])
                    .await
                {
                    tracing::error!(%error, "error updating execution");
                }
            }
            ExecutorEvent::ExecutionFailed {
                job_id,
                execution_id,
                timestamp,
                failure_reason,
                retry_policy,
                attempt_number,
            } => {
                retry_executions(
                    &*backend,
                    vec![MaybeRetryExecution {
                        execution: FailedExecution {
                            job_id,
                            execution_id,
                            failed_at: timestamp,
                            failure_reason,
                        },
                        retry_policy,
                        attempt_number,
                    }],
                )
                .await;
            }
            ExecutorEvent::ExecutorDisconnected { executor } => {
                let remaining_executions = executor.assigned_executions();

                if remaining_executions.is_empty() {
                    continue;
                }

                retry_executions(
                    &*backend,
                    remaining_executions
                        .into_iter()
                        .map(|execution| MaybeRetryExecution {
                            execution: FailedExecution {
                                job_id: execution.job_id,
                                execution_id: execution.execution_id,
                                failed_at: SystemTime::now(),
                                failure_reason: "executor disconnected".to_string(),
                            },
                            retry_policy: execution.retry_policy,
                            attempt_number: execution.attempt_number,
                        })
                        .collect(),
                )
                .await;
            }
            ExecutorEvent::JobTypesAdded { job_types } => {
                if let Err(error) = backend.add_job_types(&job_types).await {
                    tracing::error!(%error, "error adding job types to backend");
                }
            }
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

                let deadline = match execution.timeout_policy.base_time {
                    TimeoutBaseTime::TargetExecutionTime => execution
                        .target_execution_time
                        .checked_add(execution.timeout_policy.timeout)
                        .unwrap_or(SystemTime::UNIX_EPOCH),
                    TimeoutBaseTime::StartTime => execution
                        .started_at
                        .checked_add(execution.timeout_policy.timeout)
                        .unwrap_or(SystemTime::UNIX_EPOCH),
                };

                if SystemTime::now() > deadline {
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

                retry_executions(&*backend, timed_out_executions).await;
            }

            if !orphaned_executions.is_empty() {
                let execution_count = orphaned_executions.len();
                tracing::info!(execution_count, "orphaned executions found");

                retry_executions(&*backend, orphaned_executions).await;
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

async fn retry_executions<B>(backend: &B, executions: Vec<MaybeRetryExecution>)
where
    B: Backend,
{
    let mut failed_executions = Vec::new();
    let mut retried_executions = Vec::new();

    for execution in executions {
        if execution.attempt_number <= execution.retry_policy.retries {
            let backoff_duration = if execution.retry_policy.backoff_duration.is_zero() {
                execution.retry_policy.backoff_duration
            } else {
                match execution.retry_policy.backoff_strategy {
                    ora_backend::jobs::BackoffStrategy::Fixed => {
                        execution.retry_policy.backoff_duration
                    }
                    ora_backend::jobs::BackoffStrategy::Exponential => {
                        let backoff_multiplier = 2u32
                            .checked_pow(u32::try_from(execution.attempt_number).unwrap_or(2) - 1)
                            .unwrap_or(u32::MAX);

                        let mut final_backoff_duration = execution
                            .retry_policy
                            .backoff_duration
                            .checked_mul(backoff_multiplier)
                            .unwrap_or(std::time::Duration::from_secs(u64::MAX));

                        if let Some(max_backoff) = execution.retry_policy.max_backoff_duration
                            && final_backoff_duration > max_backoff
                        {
                            final_backoff_duration = max_backoff;
                        }

                        final_backoff_duration
                    }
                }
            };

            tracing::info!(
                execution_id = %execution.execution.execution_id,
                attempt_number = execution.attempt_number,
                backoff_duration = ?backoff_duration,
                "retrying execution"
            );

            retried_executions.push(RetriedExecution {
                failed_execution: execution.execution,
                retry_execution_time: SystemTime::now()
                    .checked_add(backoff_duration)
                    .unwrap_or(SystemTime::UNIX_EPOCH),
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
        if let Err(error) = backend.executions_failed(&failed_executions).await {
            tracing::error!(%error, "error updating failed executions");
        } else {
            tracing::debug!(
                count = failed_executions.len(),
                "executions reached max retry attempts"
            );
        }
    }

    if !retried_executions.is_empty()
        && let Err(error) = backend.executions_retried(&retried_executions).await
    {
        tracing::error!(%error, "error updating retried executions");
    }
}
