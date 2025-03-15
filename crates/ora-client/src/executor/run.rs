use std::{panic::AssertUnwindSafe, sync::Arc, time::UNIX_EPOCH};

use eyre::{Context, OptionExt};
use futures::FutureExt;
use ora_proto::{
    common::v1::JobType,
    server::v1::{
        executor_message::ExecutorMessageKind, server_message::ServerMessageKind,
        ExecutorCapabilities, ExecutorConnectionRequest, ExecutorConnectionResponse,
        ExecutorHeartbeat, ExecutorMessage,
    },
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;
use wgroup::WaitGroup;

#[allow(clippy::wildcard_imports)]
use tonic::codegen::*;

use crate::{executor::ExecutionContext, IndexMap};

use super::{ExecutionHandlerRaw, Executor, ExecutorOptions};

impl<C> Executor<C>
where
    C: tonic::client::GrpcService<tonic::body::BoxBody> + Clone,
    C::Error: Into<StdError>,
    C::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
    <C::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
{
    /// Run the executor until an error occurs.
    ///
    /// The error includes any errors that would prevent
    /// the executor from running, e.g. network errors.
    ///
    /// Individual task errors are not included as they are
    /// part of the job execution.
    #[tracing::instrument(skip_all, name = "executor_loop", fields(executor_id, executor_name))]
    pub async fn run(&mut self) -> eyre::Result<()> {
        let executor_span = tracing::Span::current();

        executor_span.record("executor_name", &self.options.name);

        let (executor_requests, recv) = flume::bounded(0);

        let mut state = ExecutorState {
            executor_id: None,
            options: &self.options,
            handlers: &self.handlers,
            executor_requests,
            // Start with a pessimistic heartbeat interval.
            heartbeat_interval: std::time::Duration::from_secs(1),
            in_progress_executions: Arc::new(Mutex::new(IndexMap::default())),
            wg: WaitGroup::new(),
        };

        let send_chan_guard = state.wg.add_with("send-channel");

        let mut server_messages = self
            .client
            // `Receiver::into_stream` exists, however it will cause
            // this future to be `!Send` and the compiler goes absolutely
            // bonkers about it.
            .executor_connection(tonic::Request::new(async_stream::stream!({
                loop {
                    tokio::select! {
                        _ = send_chan_guard.waiting() => {
                            tracing::debug!("send channel closed, stopping stream");
                            return;
                        }
                        msg = recv.recv_async() => {
                            if let Ok(msg) = msg {
                                tracing::trace!(?msg, "sending message");
                                yield msg;
                            } else {
                                tracing::debug!("send channel closed, stopping stream");
                                return;
                            }
                        }
                    }
                }
            })))
            .await?
            .into_inner();

        // Initial setup messages.
        state
            .executor_requests
            .send_async(ExecutorConnectionRequest {
                message: Some(ExecutorMessage {
                    executor_message_kind: Some(ExecutorMessageKind::Capabilities(
                        ExecutorCapabilities {
                            max_concurrent_executions: state
                                .options
                                .max_concurrent_executions
                                .get(),
                            name: state.options.name.clone(),
                            supported_job_types: self
                                .handlers
                                .iter()
                                .map(|h| {
                                    let handler_meta = h.job_type_metadata();

                                    JobType {
                                        id: handler_meta.id.to_string(),
                                        name: handler_meta.name.clone(),
                                        description: handler_meta.description.clone(),
                                        input_schema_json: handler_meta.input_schema_json.clone(),
                                        output_schema_json: handler_meta.output_schema_json.clone(),
                                    }
                                })
                                .collect(),
                        },
                    )),
                }),
            })
            .await?;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(state.heartbeat_interval) => {
                    if state.executor_requests.send(ExecutorConnectionRequest {
                        message: Some(ExecutorMessage {
                            executor_message_kind: Some(ExecutorMessageKind::Heartbeat(
                                ExecutorHeartbeat {},
                            )),
                        }),
                    }).is_err() {
                        return Ok(());
                    }
                }
                server_msg = server_messages.message() => {
                    match server_msg {
                        Ok(Some(server_msg)) => {
                            handle_server_response(&mut state, &executor_span, server_msg).await?;
                        }
                        Ok(None) => {
                            tracing::info!("incoming stream closed by the server");

                            if !state.in_progress_executions.lock().is_empty() {
                                tracing::warn!("cancelling executions in progress");

                                loop {
                                    let execution_state = {
                                        let mut in_progress_executions = state.in_progress_executions.lock();

                                        if in_progress_executions.is_empty() {
                                            break;
                                        }

                                        let execution_id = in_progress_executions.keys().copied().next();

                                        if let Some(execution_id) = execution_id {
                                            in_progress_executions.swap_remove(&execution_id)
                                        } else {
                                            None
                                        }
                                    };

                                    if let Some(mut execution_state) = execution_state {
                                        execution_state.cancellation_token.cancel();

                                        tokio::select! {
                                            _ = &mut execution_state.handle => {}
                                            _ = tokio::time::sleep(state.options.cancellation_grace_period) => {
                                                execution_state.handle.abort();
                                            }
                                        }
                                    } else {
                                        break;
                                    }
                                }
                            }

                            return Ok(());
                        }
                        Err(error) => {
                            tracing::warn!(?error, "received error from the server");
                        }
                    }
                }
            }
        }
    }
}

#[tracing::instrument(name = "handle_server_message", skip_all)]
async fn handle_server_response(
    state: &mut ExecutorState<'_>,
    executor_span: &tracing::Span,
    response: ExecutorConnectionResponse,
) -> eyre::Result<()> {
    let Some(message) = response.message else {
        tracing::warn!("received empty message from the server");
        return Ok(());
    };

    let Some(message) = message.server_message_kind else {
        tracing::warn!("received unknown or missing message kind from the server");
        return Ok(());
    };

    match message {
        ServerMessageKind::Properties(executor_properties) => {
            executor_span.record("executor_id", &executor_properties.executor_id);
            state.executor_id = Some(executor_properties.executor_id);

            if let Some(max_hb_interval) = executor_properties.max_heartbeat_interval {
                if let Ok(max_hb_interval) = std::time::Duration::try_from(max_hb_interval) {
                    state.heartbeat_interval = max_hb_interval / 2;
                    tracing::debug!(
                        heartbeat_interval = ?state.heartbeat_interval,
                        "using heartbeat interval"
                    );
                }
            }

            tracing::info!("received executor properties");
        }
        ServerMessageKind::ExecutionReady(execution_ready) => {
            spawn_execution(state, execution_ready).await?;
        }
        ServerMessageKind::ExecutionCancelled(execution_cancelled) => {
            let execution_id: Uuid = execution_cancelled
                .execution_id
                .parse()
                .wrap_err("expected execution ID to be UUID")?;

            let execution_state = state
                .in_progress_executions
                .lock()
                .swap_remove(&execution_id);

            if let Some(execution_state) = execution_state {
                tokio::spawn(
                    cancel_execution(execution_state, state.options.cancellation_grace_period)
                        .instrument(tracing::Span::current()),
                );
            } else {
                tracing::warn!("received cancellation for unknown execution");
            }
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all, fields(
    execution_id = %execution_state.execution_id,
))]
async fn cancel_execution(mut execution_state: ExecutionState, grace_period: std::time::Duration) {
    tracing::debug!("cancelling execution");
    execution_state.cancellation_token.cancel();

    tokio::select! {
        _ = &mut execution_state.handle => {
            tracing::debug!("execution cancelled");
        }
        _ = tokio::time::sleep(grace_period) => {
            if !execution_state.handle.is_finished() {
                tracing::warn!("execution did not cancel in time, aborting forcefully");
                execution_state.handle.abort();
            }
        }
    }

    tracing::debug!("cancelled execution");
}

#[tracing::instrument(skip_all,
    fields(
        execution_id = %execution_ready.execution_id,
        job_id = %execution_ready.job_id,
    )
)]
async fn spawn_execution(
    state: &ExecutorState<'_>,
    execution_ready: ora_proto::server::v1::ExecutionReady,
) -> eyre::Result<()> {
    let execution_span = tracing::Span::current();

    let executor_requests = state.executor_requests.clone();

    tracing::debug!("received new execution");

    let execution_id: Uuid = execution_ready
        .execution_id
        .parse()
        .wrap_err("expected execution ID to be UUID")?;

    let job_id: Uuid = execution_ready
        .job_id
        .parse()
        .wrap_err("expected job ID to be UUID")?;

    let cancellation_token = CancellationToken::new();

    let ctx = ExecutionContext {
        execution_id,
        job_id,
        target_execution_time: execution_ready
            .target_execution_time
            .and_then(|t| t.try_into().ok())
            .unwrap_or(UNIX_EPOCH),
        attempt_number: execution_ready.attempt_number,
        job_type_id: execution_ready.job_type_id,
        cancellation_token: cancellation_token.clone(),
    };

    let handler = state
        .handlers
        .iter()
        .find(|h| h.can_execute(&ctx))
        .ok_or_eyre("no handler found for the execution")?
        .clone();

    tracing::trace!("found handler for the execution");

    let now = std::time::SystemTime::now();

    if executor_requests
        .send_async(ExecutorConnectionRequest {
            message: Some(ExecutorMessage {
                executor_message_kind: Some(ExecutorMessageKind::ExecutionStarted(
                    ora_proto::server::v1::ExecutionStarted {
                        timestamp: Some(now.into()),
                        execution_id: execution_ready.execution_id,
                    },
                )),
            }),
        })
        .await
        .is_err()
    {
        tracing::debug!("not sending execution started message, executor is shutting down");
        return Ok(());
    }
    tracing::trace!("sent execution started message");

    let execution_guard = state.wg.add_with(&format!("execution-{execution_id}"));

    let cancellation_grace_period = state.options.cancellation_grace_period;

    let handle = tokio::spawn({
        let in_progress_executions = state.in_progress_executions.clone();
        let in_progress_executions2 = state.in_progress_executions.clone();
        tracing::debug!("executing handler");

        let execution_id = ctx.execution_id;

        async move {
            let mut warn_bomb = ExecutionDropWarnBomb::new(tracing::Span::current());

            let handler_fut = async move {
                match AssertUnwindSafe(handler.execute(ctx, &execution_ready.input_payload_json))
                    .catch_unwind()
                    .await
                {
                    Ok(task_result) => match task_result {
                        Ok(output_json) => {
                            tracing::debug!("execution succeeded");
                            let now = std::time::SystemTime::now();

                            if let Err(error) = executor_requests
                                .send_async(ExecutorConnectionRequest {
                                    message: Some(ExecutorMessage {
                                        executor_message_kind: Some(
                                            ExecutorMessageKind::ExecutionSucceeded(
                                                ora_proto::server::v1::ExecutionSucceeded {
                                                    timestamp: Some(now.into()),
                                                    execution_id: execution_id.to_string(),
                                                    output_payload_json: output_json,
                                                },
                                            ),
                                        ),
                                    }),
                                })
                                .await
                            {
                                tracing::warn!(?error, "failed to send execution result");
                            }
                        }
                        Err(error) => {
                            tracing::debug!(error, "execution failed");
                            let now = std::time::SystemTime::now();

                            if let Err(error) = executor_requests
                                .send_async(ExecutorConnectionRequest {
                                    message: Some(ExecutorMessage {
                                        executor_message_kind: Some(
                                            ExecutorMessageKind::ExecutionFailed(
                                                ora_proto::server::v1::ExecutionFailed {
                                                    timestamp: Some(now.into()),
                                                    execution_id: execution_id.to_string(),
                                                    error_message: error,
                                                },
                                            ),
                                        ),
                                    }),
                                })
                                .await
                            {
                                tracing::warn!(?error, "failed to send execution result");
                            }
                        }
                    },
                    Err(panic_out) => {
                        tracing::warn!("handler panicked");
                        let now = std::time::SystemTime::now();

                        let error_message = if let Some(error) = panic_out.downcast_ref::<&str>() {
                            (*error).to_string()
                        } else if let Some(error) = panic_out.downcast_ref::<String>() {
                            error.clone()
                        } else {
                            "handler panicked".to_string()
                        };

                        if let Err(error) = executor_requests
                            .send_async(ExecutorConnectionRequest {
                                message: Some(ExecutorMessage {
                                    executor_message_kind: Some(
                                        ExecutorMessageKind::ExecutionFailed(
                                            ora_proto::server::v1::ExecutionFailed {
                                                timestamp: Some(now.into()),
                                                execution_id: execution_id.to_string(),
                                                error_message,
                                            },
                                        ),
                                    ),
                                }),
                            })
                            .await
                        {
                            tracing::warn!(?error, "failed to send execution result");
                        }
                    }
                }

                if in_progress_executions
                    .lock()
                    .swap_remove(&execution_id)
                    .is_none()
                {
                    tracing::debug!(
                        "execution was not found in the in-progress list, it must have been cancelled"
                    );
                }
            };

            let mut handler_fut = std::pin::pin!(handler_fut);

            loop {
                tokio::select! {
                    _ = execution_guard.waiting() => {
                        let execution_state = in_progress_executions2.lock().swap_remove(&execution_id);

                        if let Some(execution_state) = execution_state {
                            tokio::spawn(
                                cancel_execution(execution_state, cancellation_grace_period)
                                .instrument(tracing::Span::current()));
                        }

                        (&mut handler_fut).await;
                    }
                    _ = &mut handler_fut => {
                        break;
                    }
                }
            }
            warn_bomb.defuse();
        }
        .instrument(execution_span)
    });

    state.in_progress_executions.lock().insert(
        execution_id,
        ExecutionState {
            execution_id,
            cancellation_token,
            handle,
        },
    );

    Ok(())
}

struct ExecutorState<'s> {
    executor_id: Option<String>,
    options: &'s ExecutorOptions,
    handlers: &'s [Arc<dyn ExecutionHandlerRaw + Send + Sync>],
    executor_requests: flume::Sender<ExecutorConnectionRequest>,
    heartbeat_interval: std::time::Duration,
    in_progress_executions: Arc<Mutex<IndexMap<Uuid, ExecutionState>>>,
    wg: WaitGroup,
}

struct ExecutionState {
    execution_id: Uuid,
    cancellation_token: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}

struct ExecutionDropWarnBomb {
    span: tracing::Span,
    defused: bool,
}

impl ExecutionDropWarnBomb {
    fn new(span: tracing::Span) -> Self {
        Self {
            span,
            defused: false,
        }
    }

    fn defuse(&mut self) {
        self.defused = true;
    }
}

impl Drop for ExecutionDropWarnBomb {
    fn drop(&mut self) {
        if !self.defused {
            self.span.in_scope(|| {
                tracing::warn!("execution in progess was dropped");
            });
        }
    }
}
