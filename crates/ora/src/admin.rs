//! Ora admin client.

use std::{cmp, sync::Arc, time::Duration};

use crate::{
    admin::inner::AdminClientInner, proto::admin::v1::admin_service_client::AdminServiceClient,
};

pub mod executors;
mod inner;
pub mod job_types;
pub mod jobs;
pub mod maintenance;
pub mod schedules;

/// An ergonomic client that wraps
/// an underlying gRPC admin client.
#[derive(Clone)]
#[must_use]
pub struct AdminClient {
    inner: Arc<dyn AdminClientInner>,
    poll_interval: Duration,
    caching_strategy: CachingStrategy,
}

impl std::fmt::Debug for AdminClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminClient")
            .field("poll_interval", &self.poll_interval)
            .field("caching_strategy", &self.caching_strategy)
            .finish_non_exhaustive()
    }
}

impl AdminClient {
    /// Create an admin client with the given
    /// gRPC client.
    pub fn new<T>(client: AdminServiceClient<T>) -> Self
    where
        T: Clone + Send + Sync + 'static,
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
        T::ResponseBody: http_body::Body<Data = bytes::Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as http_body::Body>::Error:
            Into<Box<dyn std::error::Error + Send + Sync + 'static>> + std::marker::Send,
        <T as tonic::client::GrpcService<tonic::body::Body>>::Future: Send,
    {
        Self {
            inner: Arc::new(client) as Arc<dyn AdminClientInner>,
            poll_interval: Duration::from_millis(500),
            caching_strategy: CachingStrategy::default(),
        }
    }

    /// Set the poll interval for long-running operations,
    /// e.g. waiting for jobs to complete.
    ///
    /// Only affects this client, however clients created
    /// from this client will inherit the value.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = cmp::max(interval, Duration::from_millis(50));
        self
    }

    /// Set the caching strategy to use
    /// when retrieving resources from the ora server.
    ///
    /// Only affects this client, however clients created
    /// from this client will inherit the value.
    pub fn with_caching_strategy(mut self, strategy: CachingStrategy) -> Self {
        self.caching_strategy = strategy;
        self
    }
}

/// The caching strategy to use when retrieving resources
/// from the ora server.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum CachingStrategy {
    /// Cache resources eagerly, even if they
    /// might not be used.
    #[default]
    Cache,
    /// Always retrieve data from the server.
    NoCache,
}
