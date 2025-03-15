use std::{
    num::NonZeroU32,
    sync::{atomic::Ordering, Arc},
    time::SystemTime,
};

use ahash::HashSet;
use eyre::{bail, Context};
use futures::StreamExt;
use ora_proto::server::v1::{
    self, executor_message::ExecutorMessageKind, ExecutorConnectionRequest,
};
use tonic::Streaming;
use uuid::Uuid;

use crate::{
    events::{EventBus, ExecutionEvent, ExecutorEvent},
    time::UnixNanos,
};

use ora_storage::{JobTimeoutBaseTime, JobType, Storage};

use super::{handle::ExecutorCapabilities, ExecutorHandle};

pub(super) async fn handle_executor_message_stream(
    backend: &impl Storage,
    executor: &ExecutorHandle,
    mut stream: Streaming<ExecutorConnectionRequest>,
    close_recv: flume::Receiver<()>,
    event_bus: EventBus,
) -> eyre::Result<()> {
    loop {
        tokio::select! {
            message = stream.next() => {
                match message {
                    Some(request) => {
                        let request = request?;

                        let Some(message) = request.message.and_then(|m| m.executor_message_kind) else {
                            bail!("missing message");
                        };

                        tracing::trace!(executor_message = ?message, "received executor message");

                        executor
                            .inner
                            .last_seen
                            .store(UnixNanos::now(), Ordering::Relaxed);

                        match message {
                            ExecutorMessageKind::Capabilities(executor_capabilities) => {
                                handle_capabilities(backend, &event_bus, executor, executor_capabilities).await?;
                            }
                            ExecutorMessageKind::Heartbeat(executor_heartbeat) => {
                                handle_heartbeat(executor, executor_heartbeat);
                            }
                            ExecutorMessageKind::ExecutionStarted(execution_started) => {
                                handle_execution_started(backend, executor, execution_started).await?;
                            }
                            ExecutorMessageKind::ExecutionSucceeded(execution_succeeded) => {
                                handle_execution_succeeded(&event_bus, backend, executor, execution_succeeded).await?;
                            }
                            ExecutorMessageKind::ExecutionFailed(execution_failed) => {
                                handle_execution_failed(&event_bus, backend, executor, execution_failed).await?;
                            }
                        }
                    },
                    None => {
                        tracing::trace!("executor stream ended");
                        return Ok(());
                    }
                }
            },
            _ = close_recv.recv_async() => {
                tracing::debug!("close signal received");
                return Ok(());
            }
        }
    }
}

#[tracing::instrument(
    name = "executor_capabilities",
    skip_all,
    fields(
        executor_name = %executor_capabilities.name
    )
)]
async fn handle_capabilities(
    storage: &impl Storage,
    event_bus: &EventBus,
    executor: &ExecutorHandle,
    executor_capabilities: v1::ExecutorCapabilities,
) -> eyre::Result<()> {
    let had_capabilities = executor.inner.capabilities.load().is_some();

    let job_types = executor_capabilities
        .supported_job_types
        .into_iter()
        .map(|job_type| JobType {
            id: job_type.id,
            name: job_type.name.unwrap_or_default(),
            description: job_type.description.unwrap_or_default(),
            input_schema_json: job_type.input_schema_json,
            output_schema_json: job_type.output_schema_json,
        })
        .collect::<Vec<_>>();

    let job_type_ids = job_types
        .iter()
        .map(|job_type| job_type.id.clone())
        .collect::<HashSet<_>>();

    storage.job_types_added(job_types).await?;

    executor
        .inner
        .capabilities
        .store(Some(Arc::new(ExecutorCapabilities {
            name: executor_capabilities.name,
            job_types: job_type_ids,
            max_concurrent_executions: NonZeroU32::new(
                executor_capabilities.max_concurrent_executions,
            ),
        })));

    tracing::info!("executor capabilities updated");
    if !had_capabilities {
        tracing::info!("executor ready");
    }

    // We emit this every time the capabilities are updated.
    event_bus.emit_executor_event(ExecutorEvent::ExecutorReady);

    Ok(())
}

#[tracing::instrument(name = "heartbeat", skip_all)]
fn handle_heartbeat(_executor: &ExecutorHandle, _executor_heartbeat: v1::ExecutorHeartbeat) {
    // no-op
}

#[tracing::instrument(
    name = "execution_started",
    skip_all,
    fields(
        execution_id = %execution_started.execution_id
    )
)]
async fn handle_execution_started(
    backend: &impl Storage,
    executor: &ExecutorHandle,
    execution_started: v1::ExecutionStarted,
) -> eyre::Result<()> {
    let execution_id = execution_started
        .execution_id
        .parse::<Uuid>()
        .wrap_err("invalid execution_id")?;

    let timestamp = execution_started
        .timestamp
        .and_then(|ts| ts.try_into().ok())
        .unwrap_or_else(SystemTime::now);

    {
        let mut executions = executor.inner.executions.write();

        let Some(execution) = executions.get_mut(&execution_id) else {
            tracing::debug!(%execution_id, "execution not found");
            return Ok(());
        };

        if execution.started_at.is_some() {
            tracing::warn!(%execution_id, "execution already started");
            return Ok(());
        }

        execution.started_at = Some(timestamp);

        if let Some(timeout) = execution.timeout_policy.timeout {
            if execution.timeout_policy.base_time == JobTimeoutBaseTime::StartTime {
                execution.timeout_deadline = Some(timestamp + timeout);
            }
        }
    }

    backend.execution_started(execution_id, timestamp).await?;

    Ok(())
}

#[tracing::instrument(
    name = "execution_succeeded",
    skip_all,
    fields(
        execution_id = %execution_succeeded.execution_id
    )
)]
async fn handle_execution_succeeded(
    event_bus: &EventBus,
    backend: &impl Storage,
    executor: &ExecutorHandle,
    execution_succeeded: v1::ExecutionSucceeded,
) -> eyre::Result<()> {
    let execution_id = execution_succeeded
        .execution_id
        .parse::<Uuid>()
        .wrap_err("invalid execution_id")?;

    let timestamp = execution_succeeded
        .timestamp
        .and_then(|ts| ts.try_into().ok())
        .unwrap_or_else(SystemTime::now);

    {
        let mut executions = executor.inner.executions.write();

        let Some(execution) = executions.get_mut(&execution_id) else {
            tracing::debug!(%execution_id, "execution not found");
            return Ok(());
        };

        if execution.started_at.is_none() {
            tracing::warn!(%execution_id, "execution was not started");
        }

        executions.remove(&execution_id);
    }

    backend
        .execution_succeeded(
            execution_id,
            timestamp,
            execution_succeeded.output_payload_json,
        )
        .await?;
    event_bus.emit_execution_event(ExecutionEvent::ExecutionsFinished);

    Ok(())
}

#[tracing::instrument(
    name = "execution_failed",
    skip_all,
    fields(
        execution_id = %execution_failed.execution_id
    )
)]
async fn handle_execution_failed(
    event_bus: &EventBus,
    backend: &impl Storage,
    executor: &ExecutorHandle,
    execution_failed: v1::ExecutionFailed,
) -> eyre::Result<()> {
    let execution_id = execution_failed
        .execution_id
        .parse::<Uuid>()
        .wrap_err("invalid execution_id")?;

    let timestamp = execution_failed
        .timestamp
        .and_then(|ts| ts.try_into().ok())
        .unwrap_or_else(SystemTime::now);

    {
        let mut executions = executor.inner.executions.write();

        let Some(execution) = executions.get_mut(&execution_id) else {
            tracing::debug!(%execution_id, "execution not found");
            return Ok(());
        };

        if execution.started_at.is_none() {
            tracing::warn!(%execution_id, "execution was not started");
        }

        executions.remove(&execution_id);
    }

    backend
        .executions_failed(
            &[execution_id],
            timestamp,
            execution_failed.error_message,
            false,
        )
        .await?;

    event_bus.emit_execution_event(ExecutionEvent::ExecutionsFinished);

    Ok(())
}
