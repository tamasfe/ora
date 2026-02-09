use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use flume::{Receiver, Sender};
use tokio::{spawn, time::timeout};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;
use wgroup::WaitGuard;

use crate::{
    execution::ExecutionId,
    executor::implementation::{
        ExecutionContext, ExecutionFailedCb, ExecutorJobQueue, capabilities::send_capabilities,
        heartbeat::heartbeat_loop,
    },
    job::JobId,
    job_type::JobTypeId,
    proto::executors::v1::{
        ExecutionFailed, ExecutionReady, ExecutionSucceeded, executor_message::ExecutorMessageKind,
        server_message::ServerMessageKind,
    },
};

#[tracing::instrument(skip_all, fields(executor_id))]
pub(super) async fn executor_loop(
    incoming: Receiver<ServerMessageKind>,
    server: Sender<ExecutorMessageKind>,
    executor_name: Arc<str>,
    queues: Arc<[ExecutorJobQueue]>,
    server_cancellation_grace_period: Duration,
    on_execution_failed: Option<ExecutionFailedCb>,
    _wg: WaitGuard,
) {
    let active_executions = ActiveExecutions::default();

    while let Ok(msg) = incoming.recv_async().await {
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
                    _ = server
                        .send_async(ExecutorMessageKind::ExecutionFailed(ExecutionFailed {
                            execution_id: execution_ready.execution_id,
                            timestamp: Some(SystemTime::now().into()),
                            failure_reason: "invalid execution ID".to_string(),
                        }))
                        .await;
                    continue;
                };

                spawn(
                    run_execution(
                        queues.clone(),
                        execution_ready,
                        server.clone(),
                        active_executions.add(execution_id),
                        server_cancellation_grace_period,
                        on_execution_failed.clone(),
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

#[tracing::instrument(skip_all, fields(execution_id, job_id, job_type_id))]
async fn run_execution(
    queues: Arc<[ExecutorJobQueue]>,
    ready_execution: ExecutionReady,
    server: Sender<ExecutorMessageKind>,
    active_execution: ActiveExecutionGuard,
    server_cancellation_grace_period: Duration,
    on_execution_failed: Option<ExecutionFailedCb>,
) {
    tracing::Span::current().record("execution_id", &ready_execution.execution_id);
    tracing::Span::current().record("job_id", &ready_execution.job_id);
    tracing::Span::current().record("job_type_id", &ready_execution.job_type_id);

    let Ok(execution_id) = ready_execution
        .execution_id
        .parse::<Uuid>()
        .map(ExecutionId)
    else {
        tracing::warn!("invalid execution id");
        _ = server
            .send_async(ExecutorMessageKind::ExecutionFailed(ExecutionFailed {
                execution_id: ready_execution.execution_id,
                timestamp: Some(SystemTime::now().into()),
                failure_reason: "invalid execution ID".to_string(),
            }))
            .await;
        return;
    };

    let Ok(job_id) = ready_execution.job_id.parse().map(JobId) else {
        tracing::warn!("invalid job id");
        _ = server
            .send_async(ExecutorMessageKind::ExecutionFailed(ExecutionFailed {
                execution_id: ready_execution.execution_id,
                timestamp: Some(SystemTime::now().into()),
                failure_reason: "invalid job ID".to_string(),
            }))
            .await;
        return;
    };

    let Ok(job_type_id) = JobTypeId::new(ready_execution.job_type_id) else {
        tracing::warn!("invalid job type id");
        _ = server
            .send_async(ExecutorMessageKind::ExecutionFailed(ExecutionFailed {
                execution_id: ready_execution.execution_id,
                timestamp: Some(SystemTime::now().into()),
                failure_reason: "invalid job type ID".to_string(),
            }))
            .await;
        return;
    };

    let Some(target_execution_time) = ready_execution
        .target_execution_time
        .and_then(|t| t.try_into().ok())
    else {
        tracing::warn!("invalid target execution time");
        _ = server
            .send_async(ExecutorMessageKind::ExecutionFailed(ExecutionFailed {
                execution_id: ready_execution.execution_id,
                timestamp: Some(SystemTime::now().into()),
                failure_reason: "invalid target execution time".to_string(),
            }))
            .await;
        return;
    };

    let Some(queue) = queues.iter().find(|q| q.job_type_id == job_type_id) else {
        tracing::warn!("job type not supported");
        _ = server
            .send_async(ExecutorMessageKind::ExecutionFailed(ExecutionFailed {
                execution_id: ready_execution.execution_id,
                timestamp: Some(SystemTime::now().into()),
                failure_reason: "job type not supported".to_string(),
            }))
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

    let mut handler_fut = (queue.handler)(ctx.clone(), ready_execution.input_payload_json);

    tokio::select! {
        handler_result = &mut handler_fut => {
            if active_execution.cancellation_token.is_cancelled() {
                return;
            }

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

                    if let Some(callback) = on_execution_failed {
                        callback(ctx, &failure_reason);
                    }

                    _ = server
                        .send_async(ExecutorMessageKind::ExecutionFailed(
                            ExecutionFailed {
                                execution_id: ready_execution.execution_id,
                                timestamp: Some(SystemTime::now().into()),
                                failure_reason,
                            },
                        ))
                        .await;
                }
            }
        }
        _ = active_execution.cancellation_token.cancelled() => {
            if timeout(server_cancellation_grace_period, handler_fut).await.is_err() {
                tracing::debug!("dropping cancelled execution");
            }
        },
    }
}

#[derive(Default)]
struct ActiveExecutions {
    executions: Arc<Mutex<HashMap<ExecutionId, CancellationToken>>>,
}

impl ActiveExecutions {
    fn add(&self, execution_id: ExecutionId) -> ActiveExecutionGuard {
        let cancellation_token = CancellationToken::new();
        let mut executions = self.executions.lock().unwrap();
        executions.insert(execution_id, cancellation_token.clone());
        ActiveExecutionGuard {
            executions: self.executions.clone(),
            this_execution_id: execution_id,
            cancellation_token,
        }
    }

    fn cancel(&self, execution_id: &ExecutionId) {
        let executions = self.executions.lock().unwrap();
        if let Some(token) = executions.get(execution_id) {
            token.cancel();
        }
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
