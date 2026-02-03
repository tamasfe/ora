//! Ora server implementation.

use std::{sync::Arc, time::Duration};

use either::Either;
use wgroup::{WaitGroup, WaitGroupHandle, WaitGuard};

use crate::{executor_pool::ExecutorPool, grpc::GrpcImpl};

mod broadcast;
mod executor_pool;
mod grpc;
pub mod proto;
mod server;
mod util;

pub use grpc::GrpcServices;
pub use ora_backend::Backend;

/// Options for configuring and spawning an Ora server.
#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// Duration after which historical data is deleted.
    ///
    /// By default, historical data is retained indefinitely.
    pub delete_history_after: Duration,
    /// Shutdown grace period after which all executions are cancelled.
    ///
    /// Defaults to 15 seconds.
    pub shutdown_grace_period: Duration,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            delete_history_after: Default::default(),
            shutdown_grace_period: Duration::from_secs(15),
        }
    }
}

/// A builder for an Ora server.
#[must_use]
pub struct ServerBuilder<B> {
    backend: Arc<B>,
    options: ServerOptions,
}

impl<B> ServerBuilder<B> {
    /// Create a new Ora server builder with the given backend.
    pub fn new(backend: B, options: ServerOptions) -> Self {
        Self {
            backend: Arc::new(backend),
            options,
        }
    }

    /// Set the duration after which historical data is deleted.
    ///
    /// By default, historical data is retained indefinitely.
    pub fn delete_history_after(mut self, duration: Duration) -> Self {
        self.options.delete_history_after = duration;
        self
    }

    /// Set the shutdown grace period after which all executions are cancelled.
    ///
    /// Defaults to 15 seconds.
    pub fn shutdown_grace_period(mut self, duration: Duration) -> Self {
        self.options.shutdown_grace_period = duration;
        self
    }
}

impl<B> ServerBuilder<B>
where
    B: Backend,
{
    /// Spawn the server, returning a handle to it.
    ///
    /// # Panics
    ///
    /// Must be called from within a Tokio runtime.
    pub fn spawn(self) -> ServerHandle<B> {
        server::spawn_server(self.backend, self.options, Either::Left(WaitGroup::new()))
    }

    /// Spawn the server using the given wait group handle.
    ///
    /// Stopping the server will be possible
    /// only via the provided wait group.
    pub fn spawn_with_wg_handle(self, wg: WaitGroupHandle) -> ServerHandle<B> {
        server::spawn_server(self.backend, self.options, Either::Right(wg))
    }
}

/// A handle to a running Ora server.
///
/// The server is automatically stopped when
/// the handle is dropped.
#[derive(Debug)]
#[must_use]
pub struct ServerHandle<B> {
    backend: Arc<B>,
    executor_pool: ExecutorPool,
    wg: Either<WaitGroup, WaitGroupHandle>,
}

impl<B> ServerHandle<B> {
    /// Stop the server gracefully.
    ///
    /// Does nothing if the server was
    /// created with an external wait group handle.
    #[allow(clippy::missing_panics_doc)]
    pub async fn stop(self) {
        if let Either::Left(wg) = self.wg {
            wg.all_done().await;
        }
    }

    // Not part of the public API.
    #[doc(hidden)]
    pub fn add_wg_internal(&self) -> WaitGuard {
        match &self.wg {
            Either::Left(wg) => wg.add(),
            Either::Right(handle) => handle.add(),
        }
    }
}

impl<B> ServerHandle<B>
where
    B: Backend,
{
    /// Return a type implementing ora gRPC services.
    #[must_use]
    pub fn grpc(&self) -> impl GrpcServices + Send + Sync + 'static {
        GrpcImpl {
            backend: self.backend.clone(),
            executor_pool: self.executor_pool.clone(),
            wg: match &self.wg {
                Either::Left(wg) => wg.handle(),
                Either::Right(handle) => handle.clone(),
            },
        }
    }
}
