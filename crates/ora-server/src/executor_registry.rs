use std::{
    num::NonZero,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use arc_swap::ArcSwapOption;
use atomic::Atomic;
use executor_messages::handle_executor_message_stream;
use futures::{stream::BoxStream, StreamExt};
use handle::{ExecutorHandle, ExecutorHandleInner};
use indexmap::IndexMap;
use ora_proto::server::v1::{
    self, executor_service_server::ExecutorService, ExecutorConnectionRequest,
    ExecutorConnectionResponse, ExecutorInfo,
};
use parking_lot::RwLock;
use tokio::spawn;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tracing::Instrument;
use uuid::Uuid;
use wgroup::WaitGroupHandle;

use crate::{
    events::{AuditEventKind, EventBus},
    time::UnixNanos,
};

use ora_storage::Storage;

mod assignment;
mod bookkeeping;
mod executor_messages;
mod handle;

/// Options for the executor registry.
#[derive(Debug, Clone)]
pub struct ExecutorRegistryOptions {
    pub executor_timeout: std::time::Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct ExecutorRegistry<S> {
    storage: Arc<S>,
    options: ExecutorRegistryOptions,
    executors: Arc<RwLock<IndexMap<Uuid, ExecutorHandle>>>,
    wg: WaitGroupHandle,
    event_bus: EventBus,
}

impl<S> ExecutorRegistry<S>
where
    S: Storage,
{
    #[must_use]
    pub fn new(
        backend: S,
        options: ExecutorRegistryOptions,
        wg: WaitGroupHandle,
        event_bus: EventBus,
    ) -> Self {
        Self {
            storage: Arc::new(backend),
            options,
            executors: Arc::new(RwLock::new(IndexMap::new())),
            wg,
            event_bus,
        }
    }

    pub(crate) fn executor_info(&self) -> Vec<ExecutorInfo> {
        self.executors
            .read()
            .values()
            .map(|e| ExecutorInfo {
                id: e.id.to_string(),
                alive: e.is_alive(),
                max_concurrent_executions: e
                    .inner
                    .capabilities
                    .load()
                    .as_ref()
                    .map(|c| c.max_concurrent_executions.map(NonZero::get).unwrap_or(0))
                    .unwrap_or(0),
                name: e
                    .inner
                    .capabilities
                    .load()
                    .as_ref()
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                supported_job_type_ids: e
                    .inner
                    .capabilities
                    .load()
                    .as_ref()
                    .map(|c| c.job_types.iter().cloned().collect())
                    .unwrap_or_default(),
                last_seen_at: Some(e.last_seen().into()),
                assigned_execution_ids: e
                    .inner
                    .executions
                    .read()
                    .keys()
                    .map(ToString::to_string)
                    .collect(),
            })
            .collect()
    }
}

#[async_trait]
impl<S> ExecutorService for ExecutorRegistry<S>
where
    S: Storage,
{
    async fn executor_connection(
        &self,
        request: Request<Streaming<ExecutorConnectionRequest>>,
    ) -> Result<Response<BoxStream<'static, Result<ExecutorConnectionResponse, Status>>>, Status>
    {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let req_stream = request.into_inner();
        let executor_id: Uuid = Uuid::now_v7();

        let wg_send = self.wg.add_with(&format!("executor-{executor_id}-send"));
        let wg_recv = self.wg.add_with(&format!("executor-{executor_id}-recv"));

        let (server_sender, server_receiver) = flume::unbounded::<ServerMessage>();
        let (close_recv_signal, close_recv) = flume::bounded::<()>(1);

        // Initial setup messages.
        server_sender
            .send(ServerMessage::V1(v1::ServerMessage {
                server_message_kind: Some(v1::server_message::ServerMessageKind::Properties(
                    v1::ExecutorProperties {
                        executor_id: executor_id.to_string(),
                        max_heartbeat_interval: Some(
                            self.options.executor_timeout.try_into().unwrap(),
                        ),
                    },
                )),
            }))
            .unwrap();

        let handle = ExecutorHandle {
            id: executor_id,
            inner: Arc::new(ExecutorHandleInner {
                last_seen: Atomic::new(UnixNanos::now()),
                recv_connected: AtomicBool::new(true),
                close_recv_signal,
                capabilities: ArcSwapOption::default(),
                sender: ArcSwapOption::new(Some(Arc::new(server_sender))),
                executions: Default::default(),
            }),
        };
        self.executors.write().insert(executor_id, handle.clone());

        tracing::info!(%executor_id, "executor connected");

        if self.event_bus.audit_events_enabled() {
            self.event_bus
                .emit_audit_event(|| AuditEventKind::ExecutorConnected { executor_id });
        }

        let backend = self.storage.clone();

        spawn({
            let event_bus = self.event_bus.clone();

            async move {
                let _wg = wg_recv;
                if let Err(error) = handle_executor_message_stream(
                    &*backend, &handle, req_stream, close_recv, event_bus,
                )
                .await
                {
                    tracing::error!(?error, "executor error");
                }

                tracing::info!("executor message stream ended");
                handle.inner.recv_connected.store(false, Ordering::Relaxed);
            }
            .instrument(tracing::info_span!("executor_message_stream", %executor_id))
        });

        let mut server_messages = server_receiver.into_stream();

        Ok(Response::new(
            async_stream::stream!({
                let _wg = wg_send;

                while let Some(message) = server_messages.next().await {
                    match message {
                        ServerMessage::V1(message) => {
                            yield Ok(ExecutorConnectionResponse {
                                message: Some(message),
                            });
                        }
                        ServerMessage::Error(status) => {
                            yield Err(status);
                        }
                    }
                }
            })
            .boxed(),
        ))
    }
}

#[derive(Debug)]
enum ServerMessage {
    V1(v1::ServerMessage),
    #[allow(dead_code)]
    Error(Status),
}

impl From<v1::ServerMessage> for ServerMessage {
    fn from(v: v1::ServerMessage) -> Self {
        Self::V1(v)
    }
}
