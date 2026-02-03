//! Server implementation and utilities.

use ora_server::proto::{admin::v1::admin_service_server::AdminServiceServer, executors::v1::execution_service_server::ExecutionServiceServer};
use tonic::transport::Channel;

use crate::proto::{
    admin::v1::admin_service_client::AdminServiceClient,
    executors::v1::execution_service_client::ExecutionServiceClient,
};

pub use ora_server::{Backend, GrpcServices, ServerBuilder, ServerHandle};

/// Extension trait for an ora server handle.
pub trait ServerHandleExt {
    /// Get an admin service client that connects to the server
    /// using an in-memory transport.
    ///
    /// Make sure to reuse the client as every call to this function
    /// will spawn a new task that is only cleaned up
    /// once the server is dropped.
    fn admin_client(&self) -> AdminServiceClient<Channel>;
    /// Get an execution service client that connects to the server
    /// using an in-memory transport.
    ///
    /// Make sure to reuse the client as every call to this function
    /// will spawn a new task that is only cleaned up
    /// once the server is dropped.
    fn execution_client(&self) -> ExecutionServiceClient<Channel>;
}

impl<B> ServerHandleExt for ora_server::ServerHandle<B>
where
    B: Backend,
{
    fn admin_client(&self) -> AdminServiceClient<Channel> {
        use hyper_util::rt::TokioIo;
        use tonic::transport::{Endpoint, Uri};

        let (client, server) = tokio::io::duplex(1024);

        let admin = self.grpc();
        let wg = self.add_wg_internal();

        tokio::spawn(async move {
            let srv = tonic::transport::Server::builder()
                .add_service(AdminServiceServer::new(admin).max_decoding_message_size(usize::MAX))
                .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)));

            let waiting = wg.waiting();

            tokio::select! {
                _ = waiting => {}
                serve_result = srv => {
                    if let Err(error) = serve_result {
                        tracing::error!(?error, "error during admin service serve");
                    }
                }
            }
        });

        let mut client = Some(client);
        let channel = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector_lazy(tower::service_fn(move |_: Uri| {
                let client = client.take();

                async move {
                    if let Some(client) = client {
                        Ok(TokioIo::new(client))
                    } else {
                        Err(std::io::Error::other("Client already taken"))
                    }
                }
            }));

        AdminServiceClient::new(channel)
    }

    fn execution_client(&self) -> ExecutionServiceClient<Channel> {
        use hyper_util::rt::TokioIo;
        use tonic::transport::{Endpoint, Uri};

        let (client, server) = tokio::io::duplex(1024);

        let admin = self.grpc();
        let wg = self.add_wg_internal();

        tokio::spawn(async move {
            let srv = tonic::transport::Server::builder()
                .add_service(ExecutionServiceServer::new(admin).max_decoding_message_size(usize::MAX))
                .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)));

            let waiting = wg.waiting();

            tokio::select! {
                _ = waiting => {}
                serve_result = srv => {
                    if let Err(error) = serve_result {
                        tracing::error!(?error, "error during admin service serve");
                    }
                }
            }
        });

        let mut client = Some(client);
        let channel = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector_lazy(tower::service_fn(move |_: Uri| {
                let client = client.take();

                async move {
                    if let Some(client) = client {
                        Ok(TokioIo::new(client))
                    } else {
                        Err(std::io::Error::other("Client already taken"))
                    }
                }
            }));

        ExecutionServiceClient::new(channel)
    }
}
