//! A minimal executor that talks to the server directly
//! over the `ExecutionService` gRPC stream.
//!
//! It timestamps every server message as soon as it arrives
//! and reports it to the running scenario as an [`Event`].

use std::time::{Duration, Instant, SystemTime};

use eyre::{Context, OptionExt};
use futures::StreamExt;
use ora::proto::{
    executors::v1::{
        ExecutionAccepted, ExecutionFailed, ExecutionReady, ExecutionSucceeded,
        ExecutorCapabilities, ExecutorConnectionRequest, ExecutorHeartbeat, ExecutorJobQueue,
        ExecutorMessage, execution_service_client::ExecutionServiceClient,
        executor_message::ExecutorMessageKind, server_message::ServerMessageKind,
    },
    jobs::v1::JobType,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tonic::transport::Channel;

/// The job type handled by the benchmark executor.
pub const JOB_TYPE_ID: &str = "ora_bench.Job";

/// What the benchmark executor should do with a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Succeed immediately.
    Succeed,
    /// Fail the first attempt, succeed on subsequent attempts.
    FailFirst,
    /// Never complete, wait for cancellation.
    Hold,
}

/// The input payload of benchmark jobs.
#[derive(Debug, Serialize, Deserialize)]
pub struct Payload {
    /// What to do with the job.
    pub mode: Mode,
}

/// An event observed by the benchmark executor.
#[derive(Debug)]
pub enum Event {
    /// An execution was received from the server.
    Ready {
        job_id: String,
        execution_id: String,
        attempt: u64,
        /// When the message was received (monotonic).
        at: Instant,
        /// When the message was received (wall clock).
        wall_at: SystemTime,
    },
    /// A failure was sent to the server for the given execution.
    FailedSent { job_id: String, at: Instant },
    /// The server cancelled an execution.
    Cancelled { execution_id: String, at: Instant },
}

/// A connected benchmark executor.
///
/// The connection is dropped abruptly once this is dropped,
/// use [`BenchExecutor::shutdown`] to close it gracefully.
pub struct BenchExecutor {
    /// The ID assigned by the server.
    pub executor_id: String,
    stop: oneshot::Sender<()>,
    connection: JoinHandle<()>,
}

impl BenchExecutor {
    /// End the executor stream and wait for the server to close the connection.
    pub async fn shutdown(self) {
        _ = self.stop.send(());
        if tokio::time::timeout(Duration::from_secs(5), self.connection)
            .await
            .is_err()
        {
            tracing::warn!("timed out waiting for the executor connection to close");
        }
    }
}

/// Connect a benchmark executor to the server.
pub async fn connect(
    channel: Channel,
    capacity: u64,
) -> eyre::Result<(BenchExecutor, mpsc::UnboundedReceiver<Event>)> {
    let mut client = ExecutionServiceClient::new(channel);

    let (outgoing, outgoing_recv) = mpsc::unbounded_channel();
    let (events, events_recv) = mpsc::unbounded_channel();
    let (stop, stop_recv) = oneshot::channel::<()>();

    outgoing
        .send(ExecutorMessageKind::Capabilities(ExecutorCapabilities {
            name: "ora-bench".into(),
            job_queues: vec![ExecutorJobQueue {
                job_type: Some(JobType {
                    id: JOB_TYPE_ID.into(),
                    description: Some("Job type used by ora-bench.".into()),
                    input_schema_json: None,
                    output_schema_json: None,
                }),
                max_concurrent_executions: capacity,
            }],
            execution_handshake: true,
        }))
        .ok()
        .ok_or_eyre("executor channel closed")?;

    let mut incoming = client
        .executor_connection(
            UnboundedReceiverStream::new(outgoing_recv)
                .take_until(stop_recv)
                .map(|kind| ExecutorConnectionRequest {
                    message: Some(ExecutorMessage {
                        executor_message_kind: Some(kind),
                    }),
                }),
        )
        .await
        .wrap_err("failed to open executor connection")?
        .into_inner();

    let (props_send, props_recv) = oneshot::channel();

    let connection = tokio::spawn({
        let outgoing = outgoing.clone();
        async move {
            let mut props_send = Some(props_send);

            loop {
                let msg = match incoming.message().await {
                    Ok(Some(res)) => res.message.and_then(|m| m.server_message_kind),
                    Ok(None) => {
                        tracing::debug!("executor connection closed by the server");
                        break;
                    }
                    Err(error) => {
                        tracing::error!(%error, "executor connection failed");
                        break;
                    }
                };

                match msg {
                    Some(ServerMessageKind::ExecutionReady(ready)) => {
                        handle_ready(ready, &outgoing, &events);
                    }
                    Some(ServerMessageKind::ExecutionCancelled(cancelled)) => {
                        _ = events.send(Event::Cancelled {
                            execution_id: cancelled.execution_id,
                            at: Instant::now(),
                        });
                    }
                    Some(ServerMessageKind::Properties(props)) => {
                        if let Some(send) = props_send.take() {
                            _ = send.send(props);
                        }
                    }
                    None => {
                        tracing::warn!("received empty server message");
                    }
                }
            }
        }
    });

    let props = tokio::time::timeout(Duration::from_secs(10), props_recv)
        .await
        .wrap_err("timed out waiting for executor properties")?
        .wrap_err("executor connection closed before receiving properties")?;

    let heartbeat_interval = props
        .max_heartbeat_interval
        .and_then(|d| Duration::try_from(d).ok())
        .unwrap_or(Duration::from_secs(5))
        / 2;

    tokio::spawn({
        async move {
            while outgoing
                .send(ExecutorMessageKind::Heartbeat(ExecutorHeartbeat {}))
                .is_ok()
            {
                tokio::time::sleep(heartbeat_interval).await;
            }
        }
    });

    Ok((
        BenchExecutor {
            executor_id: props.executor_id,
            stop,
            connection,
        },
        events_recv,
    ))
}

fn handle_ready(
    ready: ExecutionReady,
    outgoing: &mpsc::UnboundedSender<ExecutorMessageKind>,
    events: &mpsc::UnboundedSender<Event>,
) {
    let at = Instant::now();
    let wall_at = SystemTime::now();

    let mode = serde_json::from_str::<Payload>(&ready.input_payload_json)
        .map_or(Mode::Succeed, |p| p.mode);

    _ = events.send(Event::Ready {
        job_id: ready.job_id.clone(),
        execution_id: ready.execution_id.clone(),
        attempt: ready.attempt_number,
        at,
        wall_at,
    });

    _ = outgoing.send(ExecutorMessageKind::ExecutionAccepted(ExecutionAccepted {
        execution_id: ready.execution_id.clone(),
        timestamp: Some(SystemTime::now().into()),
    }));

    match mode {
        Mode::FailFirst if ready.attempt_number <= 1 => {
            let at = Instant::now();
            _ = outgoing.send(ExecutorMessageKind::ExecutionFailed(ExecutionFailed {
                execution_id: ready.execution_id,
                timestamp: Some(SystemTime::now().into()),
                failure_reason: "ora-bench: failing first attempt".into(),
            }));
            _ = events.send(Event::FailedSent {
                job_id: ready.job_id,
                at,
            });
        }
        Mode::Succeed | Mode::FailFirst => {
            _ = outgoing.send(ExecutorMessageKind::ExecutionSucceeded(
                ExecutionSucceeded {
                    execution_id: ready.execution_id,
                    timestamp: Some(SystemTime::now().into()),
                    output_payload_json: "null".into(),
                },
            ));
        }
        Mode::Hold => {}
    }
}
