use futures::{stream::BoxStream, StreamExt, TryStreamExt};
use ora_proto::snapshot::v1::snapshot_service_server::SnapshotService;
use tonic::{async_trait, Response, Status};
use wgroup::WaitGroupHandle;

use crate::{events::EventBus, AuditEventKind};

use ora_storage::{Storage, StorageSnapshot};

/// A simple wrapper around a storage backend
/// that provides a snapshot interface.
pub struct SnapshotInterface<S> {
    storage: S,
    wg: WaitGroupHandle,
    event_bus: EventBus,
}

impl<S> SnapshotInterface<S> {
    /// Create a new snapshot interface.
    pub fn new(storage: S, wg: WaitGroupHandle, event_bus: EventBus) -> Self {
        Self {
            storage,
            wg,
            event_bus,
        }
    }
}

#[async_trait]
impl<S> SnapshotService for SnapshotInterface<S>
where
    S: Storage + StorageSnapshot,
{
    async fn export(
        &self,
        _request: tonic::Request<ora_proto::snapshot::v1::ExportRequest>,
    ) -> std::result::Result<
        tonic::Response<
            BoxStream<'static, Result<ora_proto::snapshot::v1::ExportResponse, Status>>,
        >,
        Status,
    > {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let snapshot_data = self.storage.export_snapshot();

        self.event_bus
            .emit_audit_event(|| AuditEventKind::SnapshotExported);

        Ok(Response::new(
            snapshot_data
                .map_err(|error| {
                    tracing::error!(?error, "failed to export snapshot");
                    Status::internal("failed to export snapshot")
                })
                .map_ok(|data| ora_proto::snapshot::v1::ExportResponse { data: data.into() })
                .boxed(),
        ))
    }

    async fn import(
        &self,
        request: tonic::Request<tonic::Streaming<ora_proto::snapshot::v1::ImportRequest>>,
    ) -> std::result::Result<tonic::Response<ora_proto::snapshot::v1::ImportResponse>, Status> {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        self.event_bus
            .emit_audit_event(|| AuditEventKind::SnapshotImported);

        self.storage
            .import_snapshot(
                request
                    .into_inner()
                    .map_ok(|req| req.data.unwrap())
                    .map_err(|e| {
                        tracing::error!("failed to read import request: {:?}", e);
                        eyre::eyre!(e)
                    })
                    .boxed(),
            )
            .await
            .map_err(|error| {
                tracing::error!(?error, "failed to import snapshot");
                Status::internal("failed to import snapshot")
            })?;

        Ok(Response::new(ora_proto::snapshot::v1::ImportResponse {}))
    }
}
