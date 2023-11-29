//! A registry that does nothing.

use std::convert::Infallible;

use async_trait::async_trait;

use super::{HeartbeatResponse, WorkerRegistry};

/// A worker registry that does nothing.
#[derive(Debug, Default, Clone)]
pub struct NoopWorkerRegistry;

#[async_trait]
impl WorkerRegistry for NoopWorkerRegistry {
    type Error = Infallible;

    async fn register_worker(
        &self,
        _worker_id: uuid::Uuid,
        _metadata: &super::WorkerMetadata,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn unregister_worker(&self, _worker_id: uuid::Uuid) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn heartbeat(
        &self,
        _worker_id: uuid::Uuid,
        _data: &super::HeartbeatData,
    ) -> Result<HeartbeatResponse, Self::Error> {
        Ok(HeartbeatResponse {
            should_register: false,
        })
    }

    async fn workers(&self) -> Result<Vec<super::WorkerInfo>, Self::Error> {
        Ok(Vec::new())
    }

    fn enabled(&self) -> bool {
        false
    }
}
