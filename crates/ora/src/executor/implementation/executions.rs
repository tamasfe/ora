use std::{
    cmp,
    collections::HashMap,
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use flume::{Receiver, Sender};
use futures::FutureExt;
use tokio::{select, spawn, time::timeout};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;
use wgroup::WaitGuard;

use crate::{
    execution::ExecutionId,
    executor::implementation::{
        Admission, ExecutionContext, ExecutionFailedCb, ExecutionGuard, ExecutorJobQueue,
        capabilities::send_capabilities, heartbeat::heartbeat_loop,
    },
    job::JobId,
    job_type::JobTypeId,
    proto::executors::v1::{
        ExecutionAccepted, ExecutionFailed, ExecutionReady, ExecutionRejected, ExecutionSucceeded,
        executor_message::ExecutorMessageKind, server_message::ServerMessageKind,
    },
};

/// The maximum time execution guards can take
/// before the execution is rejected.
///
/// This must be shorter than the time the server waits
/// for executions to be accepted.
const EXECUTION_GUARD_TIMEOUT: Duration = Duration::from_secs(10);

#[tracing::instrument(skip_all, fields(executor_id))]
pub(super) async fn executor_loop(
    incoming: Receiver<ServerMessageKind>,
    server: Sender<ExecutorMessageKind>,
    executor_name: Arc<str>,
    queues: Arc<[ExecutorJobQueue]>,
    server_cancellation_grace_period: Duration,
    on_execution_failed: Option<ExecutionFailedCb>,
    execution_guard: Option<ExecutionGuard>,
    wg: WaitGuard,
) {
    let active_executions = ActiveExecutions::default();

    loop {
        let msg = select! {
                biased;

                _ = wg.waiting() => {
                    tracing::debug!("executor loop is shutting down");
                    if active_executions.is_empty() {
                        tracing::debug!("no active executions, shutting down immediately");
                        break;
                    }

                    active_executions.cancel_all();
                    break;
                }
                msg = incoming.recv_async() => {
                    match msg {
                        Ok(msg) => msg,
                        Err(_) => {
                            tracing::debug!("server channel closed, shutting down executor loop");
                            // The server considers the executions failed
                            // and might retry them elsewhere.
                            active_executions.cancel_all();
                            break;
                        }
                    }
            }
        };

        match msg {
            ServerMessageKind::Properties(executor_properties) => {
                tracing::Span::current().record("executor_id", &executor_properties.executor_id);

                let mut max_heartbeat_interval = Duration::try_from(
                    executor_properties
                        .max_heartbeat_interval
                        .unwrap_or_default(),
                )
                .unwrap_or_default();

                if max_heartbeat_interval.is_zero() {
                    max_heartbeat_interval = Duration::from_secs(1);
                    tracing::warn!("invalid heartbeat interval");
                }

                if let Err(error) = send_capabilities(&executor_name, &queues, &server).await {
                    tracing::error!(?error, "failed to send executor capabilities");
                    break;
                }

                spawn(heartbeat_loop(max_heartbeat_interval, server.clone()));

                tracing::info!("executor initialized");
            }
            ServerMessageKind::ExecutionReady(execution_ready) => {
                let Ok(execution_id) = execution_ready
                    .execution_id
                    .parse::<Uuid>()
                    .map(ExecutionId)
                else {
                    tracing::error!("invalid execution id");
                    reject(
                        &server,
                        execution_ready.execution_id,
                        Some("invalid execution ID".to_string()),
                    )
                    .await;
                    continue;
                };

                // An execution offered again while it is still running here
                // (e.g. after it was cancelled) would share its tracking
                // with the earlier run, so it is left to other executors.
                let Some(active_execution) = active_executions.add(execution_id) else {
                    tracing::warn!(%execution_id, "execution offered while it is still running");
                    reject(
                        &server,
                        execution_ready.execution_id,
                        Some("execution is already running".to_string()),
                    )
                    .await;
                    continue;
                };

                spawn(
                    run_execution(
                        queues.clone(),
                        execution_ready,
                        server.clone(),
                        active_execution,
                        execution_guard.clone(),
                        server_cancellation_grace_period,
                        on_execution_failed.clone(),
                        wg.add_with("execution"),
                    )
                    .in_current_span(),
                );
            }
            ServerMessageKind::ExecutionCancelled(execution_cancelled) => {
                let Ok(execution_id) = execution_cancelled
                    .execution_id
                    .parse::<Uuid>()
                    .map(ExecutionId)
                else {
                    tracing::error!("invalid execution id for cancellation");
                    continue;
                };

                active_executions.cancel(&execution_id);
            }
        }
    }
}

/// Reject an execution offered by the server.
async fn reject(
    server: &Sender<ExecutorMessageKind>,
    execution_id: String,
    reason: Option<String>,
) {
    tracing::debug!(
        reason = reason.as_deref().unwrap_or_default(),
        "rejecting execution"
    );
    _ = server
        .send_async(ExecutorMessageKind::ExecutionRejected(ExecutionRejected {
            execution_id,
            timestamp: Some(SystemTime::now().into()),
            reason,
        }))
        .await;
}

/// Accept (or reject) an execution offered by the server,
/// and run it if it was accepted.
#[tracing::instrument(skip_all, fields(execution_id, job_id, job_type_id))]
#[allow(clippy::too_many_arguments)]
async fn run_execution(
    queues: Arc<[ExecutorJobQueue]>,
    ready_execution: ExecutionReady,
    server: Sender<ExecutorMessageKind>,
    active_execution: ActiveExecutionGuard,
    execution_guard: Option<ExecutionGuard>,
    server_cancellation_grace_period: Duration,
    on_execution_failed: Option<ExecutionFailedCb>,
    _wg: WaitGuard,
) {
    tracing::Span::current().record("execution_id", &ready_execution.execution_id);
    tracing::Span::current().record("job_id", &ready_execution.job_id);
    tracing::Span::current().record("job_type_id", &ready_execution.job_type_id);

    let execution_id = active_execution.this_execution_id;

    let Ok(job_id) = ready_execution.job_id.parse().map(JobId) else {
        tracing::warn!("invalid job id");
        reject(
            &server,
            ready_execution.execution_id,
            Some("invalid job ID".to_string()),
        )
        .await;
        return;
    };

    let Ok(job_type_id) = JobTypeId::new(ready_execution.job_type_id) else {
        tracing::warn!("invalid job type id");
        reject(
            &server,
            ready_execution.execution_id,
            Some("invalid job type ID".to_string()),
        )
        .await;
        return;
    };

    let Some(target_execution_time) = ready_execution
        .target_execution_time
        .and_then(|t| t.try_into().ok())
    else {
        tracing::warn!("invalid target execution time");
        reject(
            &server,
            ready_execution.execution_id,
            Some("invalid target execution time".to_string()),
        )
        .await;
        return;
    };

    let Some(queue) = queues.iter().find(|q| q.job_type_id == job_type_id) else {
        tracing::warn!("job type not supported");
        reject(
            &server,
            ready_execution.execution_id,
            Some("job type not supported".to_string()),
        )
        .await;
        return;
    };

    // The server should not offer more executions than the executor can handle,
    // but executions from earlier connections are not known to it.
    let Some(_slot) = QueueSlot::reserve(&queue.active_jobs, queue.max_concurrent_jobs) else {
        reject(
            &server,
            ready_execution.execution_id,
            Some("executor at capacity".to_string()),
        )
        .await;
        return;
    };

    let ctx = ExecutionContext {
        execution_id,
        job_id,
        job_type_id,
        target_execution_time,
        attempt_number: ready_execution.attempt_number,
        cancellation_token: active_execution.cancellation_token.clone(),
    };

    // The executor's guard is run first, the handler's guard
    // is only run if the executor's guard accepted the execution.
    let admission = async {
        for guard in [&execution_guard, &queue.execution_guard]
            .into_iter()
            .flatten()
        {
            let admission = AssertUnwindSafe(guard(ctx.clone()))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| Admission::reject_with("execution guard panicked"));

            if !admission.is_accept() {
                return admission;
            }
        }

        Admission::Accept
    };

    let admission = tokio::select! {
        admission = timeout(EXECUTION_GUARD_TIMEOUT, admission) => {
            admission.unwrap_or_else(|_| Admission::reject_with("execution guard timed out"))
        },
        () = active_execution.cancellation_token.cancelled() => {
            // The offer was withdrawn by the server (or the executor is shutting down).
            tracing::debug!("execution cancelled before it was accepted");
            return;
        }
    };

    if let Admission::Reject { reason } = admission {
        reject(&server, ready_execution.execution_id, reason).await;
        return;
    }

    if server
        .send_async(ExecutorMessageKind::ExecutionAccepted(ExecutionAccepted {
            execution_id: ready_execution.execution_id.clone(),
            timestamp: Some(SystemTime::now().into()),
        }))
        .await
        .is_err()
    {
        tracing::debug!("server channel closed, not running execution");
        return;
    }

    // A panicking handler is reported as a failure,
    // otherwise the server would wait for its result indefinitely.
    let mut handler_fut = AssertUnwindSafe((queue.handler)(
        ctx.clone(),
        ready_execution.input_payload_json,
    ))
    .catch_unwind()
    .map(|result| {
        result.unwrap_or_else(|panic| {
            let message = panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic payload");

            Err(eyre::eyre!("handler panicked: {message}"))
        })
    });

    let handler_result = tokio::select! {
        handler_result = &mut handler_fut => handler_result,
        _ = active_execution.cancellation_token.cancelled() => {
            match timeout(server_cancellation_grace_period, handler_fut).await {
                // The result is still reported, the server ignores it
                // if it cancelled the execution, but the cancellation
                // might have been caused by the executor shutting down.
                Ok(handler_result) => handler_result,
                Err(_) => {
                    tracing::debug!("dropping cancelled execution");
                    return;
                }
            }
        },
    };

    match handler_result {
        Ok(output_payload_json) => {
            _ = server
                .send_async(ExecutorMessageKind::ExecutionSucceeded(
                    ExecutionSucceeded {
                        execution_id: ready_execution.execution_id,
                        timestamp: Some(SystemTime::now().into()),
                        output_payload_json,
                    },
                ))
                .await;
        }
        Err(error) => {
            let failure_reason = format!("{error:?}");

            if let Some(callback) = on_execution_failed
                && !active_execution.cancellation_token.is_cancelled()
            {
                // The failure must be reported even if the callback panics.
                if std::panic::catch_unwind(AssertUnwindSafe(|| callback(ctx, &failure_reason)))
                    .is_err()
                {
                    tracing::error!("execution failure callback panicked");
                }
            }

            _ = server
                .send_async(ExecutorMessageKind::ExecutionFailed(ExecutionFailed {
                    execution_id: ready_execution.execution_id,
                    timestamp: Some(SystemTime::now().into()),
                    failure_reason,
                }))
                .await;
        }
    }
}

#[derive(Default)]
struct ActiveExecutions {
    executions: Arc<Mutex<HashMap<ExecutionId, CancellationToken>>>,
}

impl ActiveExecutions {
    /// Track a new active execution.
    ///
    /// Returns `None` if the execution is already active.
    fn add(&self, execution_id: ExecutionId) -> Option<ActiveExecutionGuard> {
        let cancellation_token = CancellationToken::new();
        let mut executions = self.executions.lock().unwrap();

        if executions.contains_key(&execution_id) {
            return None;
        }

        executions.insert(execution_id, cancellation_token.clone());
        Some(ActiveExecutionGuard {
            executions: self.executions.clone(),
            this_execution_id: execution_id,
            cancellation_token,
        })
    }

    fn cancel(&self, execution_id: &ExecutionId) {
        let executions = self.executions.lock().unwrap();
        if let Some(token) = executions.get(execution_id) {
            token.cancel();
        }
    }

    fn is_empty(&self) -> bool {
        let executions = self.executions.lock().unwrap();
        executions.is_empty()
    }

    fn cancel_all(&self) {
        let executions = self.executions.lock().unwrap();
        for token in executions.values() {
            token.cancel();
        }
    }
}

/// A reserved slot of a job queue, released when dropped.
struct QueueSlot<'a> {
    active_jobs: &'a AtomicU64,
}

impl<'a> QueueSlot<'a> {
    fn reserve(active_jobs: &'a AtomicU64, max_concurrent_jobs: u64) -> Option<Self> {
        let max_concurrent_jobs = cmp::max(max_concurrent_jobs, 1);

        active_jobs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < max_concurrent_jobs).then_some(active + 1)
            })
            .ok()?;

        Some(Self { active_jobs })
    }
}

impl Drop for QueueSlot<'_> {
    fn drop(&mut self) {
        self.active_jobs.fetch_sub(1, Ordering::AcqRel);
    }
}

struct ActiveExecutionGuard {
    executions: Arc<Mutex<HashMap<ExecutionId, CancellationToken>>>,
    this_execution_id: ExecutionId,
    cancellation_token: CancellationToken,
}

impl Drop for ActiveExecutionGuard {
    fn drop(&mut self) {
        let mut executions = self.executions.lock().unwrap();
        executions.remove(&self.this_execution_id);
    }
}
