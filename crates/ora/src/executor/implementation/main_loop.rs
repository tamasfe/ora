use std::{pin::pin, sync::Arc};

use tokio::spawn;
use wgroup::{WaitGroup, WaitGroupHandle};

use crate::executor::implementation::{
    Executor, ExecutorHandle, ExecutorJobQueue, connection::connection_loop,
    executions::executor_loop,
};

impl<T> Executor<T>
where
    T: tonic::client::GrpcService<tonic::body::Body> + Send + 'static,
    T::Error: Into<tonic::codegen::StdError>,
    T::ResponseBody:
        tonic::codegen::Body<Data = tonic::codegen::Bytes> + std::marker::Send + 'static,
    <T::ResponseBody as tonic::codegen::Body>::Error:
        Into<tonic::codegen::StdError> + std::marker::Send,
    <T as tonic::client::GrpcService<tonic::body::Body>>::Future: std::marker::Send,
{
    /// Spawn the executor on the tokio runtime.
    ///
    /// The executor will run until it is stopped
    /// and will attempt to handle all errors, including
    /// reconnection in case the connection is lost with the server.
    pub fn spawn(self) -> ExecutorHandle {
        self.spawn_impl(None)
    }

    /// Spawn the executor on the tokio runtime.
    ///
    /// The executor will run until it is stopped
    /// and will attempt to handle all errors, including
    /// reconnection in case the connection is lost with the server.
    ///
    /// The executor will use the provided wait group for shutdown.
    pub fn spawn_with_wg(self, wg_handle: WaitGroupHandle) {
        _ = self.spawn_impl(Some(wg_handle));
    }

    fn spawn_impl(self, wg_handle: Option<WaitGroupHandle>) -> ExecutorHandle {
        let wg = if wg_handle.is_none() {
            Some(WaitGroup::new())
        } else {
            None
        };

        let executor_wg = match (wg_handle, &wg) {
            (Some(handle), None) => handle.add_with("executor"),
            (None, Some(wg)) => wg.add_with("executor"),
            _ => unreachable!(),
        };
        let running_executions_group = WaitGroup::new();

        spawn(async move {
            let mut client = self.client;
            let queues = Arc::<[ExecutorJobQueue]>::from(self.queues);
            let executor_name = Arc::<str>::from(self.options.name.as_str());

            loop {
                let (out_send, out_recv) = flume::unbounded();
                let (in_send, in_recv) = flume::unbounded();

                spawn(executor_loop(
                    in_recv,
                    out_send,
                    executor_name.clone(),
                    queues.clone(),
                    self.options.cancellation_grace_period,
                    self.on_execution_failed.clone(),
                    running_executions_group.add(),
                ));

                let mut conn_loop = pin!(connection_loop(&mut client, in_send, out_recv));

                tokio::select! {
                    _ = &mut conn_loop => {},
                    _ = executor_wg.waiting() => {
                        tracing::info!("executor is shutting down...");
                        running_executions_group.all_done().await;
                        return;
                    }
                }

                tracing::warn!("reconnecting in 5 seconds...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });

        ExecutorHandle { _wg: wg }
    }
}
