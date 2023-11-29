#![allow(missing_docs)]

use std::collections::BTreeMap;

use async_trait::async_trait;
use ora_common::{
    task::{TaskDataFormat, TaskMetadata, WorkerSelector},
    timeout::TimeoutPolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

pub mod noop;

/// The worker registry is an optional component that keeps
/// track of workers that are currently available along with
/// additional information, such as tasks they support.
///
/// The registry is not involved in the task scheduling and
/// execution process, the workers themselves are responsible for
/// claiming tasks and reporting their status to the registry.
#[async_trait]
pub trait WorkerRegistry {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Register a worker with the registry.
    async fn register_worker(
        &self,
        worker_id: Uuid,
        metadata: &WorkerMetadata,
    ) -> Result<(), Self::Error>;
    /// Unregister a worker from the registry.
    async fn unregister_worker(&self, worker_id: Uuid) -> Result<(), Self::Error>;
    /// Send a heartbeat to the registry for the given worker.
    async fn heartbeat(
        &self,
        worker_id: Uuid,
        data: &HeartbeatData,
    ) -> Result<HeartbeatResponse, Self::Error>;
    /// Get information about all workers.
    async fn workers(&self) -> Result<Vec<WorkerInfo>, Self::Error>;

    /// Suggested heartbeat interval for workers.
    fn heartbeat_interval(&self) -> time::Duration {
        time::Duration::seconds(30)
    }
    /// Check if the registry is enabled,
    /// this is an optimization to avoid
    /// unnecessary work.
    fn enabled(&self) -> bool {
        true
    }
}

/// Metadata sent by the worker when registering with the registry.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WorkerMetadata {
    /// The name of the worker.
    pub name: Option<String>,
    /// Additional worker description.
    pub description: Option<String>,
    /// The version of the worker.
    pub version: Option<String>,
    /// Supported tasks.
    pub supported_tasks: Vec<SupportedTask>,
    /// Arbitrary fields.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Task information supported by the worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportedTask {
    /// The worker selector of the task that will
    /// be matched to this worker.
    pub worker_selector: WorkerSelector,
    /// The data format of the task input and output.
    pub default_data_format: TaskDataFormat,
    /// The default timeout policy.
    pub default_timeout: TimeoutPolicy,
    /// Additional task metadata.
    pub metadata: TaskMetadata,
}

/// Information about a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    /// The worker ID.
    pub id: Uuid,
    /// The worker metadata.
    pub metadata: WorkerMetadata,
    /// The time of the first registration.
    #[serde(with = "time::serde::rfc3339")]
    pub registered: OffsetDateTime,
    /// The time of registration or last heartbeat.
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen: OffsetDateTime,
}

/// Data sent in a heartbeat request by the worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatData {}

/// A response to a heartbeat request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    /// The worker should register itself,
    /// because it is not known to the registry.
    ///
    /// This can happen if the registry has no
    /// persistent storage and was restarted.
    pub should_register: bool,
}
