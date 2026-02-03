use futures::{StreamExt, future::ready, stream::BoxStream};
use ora_backend::Backend;
use tonic::{Request, Response, Status};

use crate::{
    grpc::GrpcImpl,
    proto::executors::v1::{self, execution_service_server::ExecutionService},
};

#[tonic::async_trait]
impl<B> ExecutionService for GrpcImpl<B>
where
    B: Backend,
{
    async fn executor_connection(
        &self,
        request: Request<tonic::Streaming<v1::ExecutorConnectionRequest>>,
    ) -> Result<Response<BoxStream<'static, tonic::Result<v1::ExecutorConnectionResponse>>>, Status>
    {
        if self.wg.is_waiting() {
            return Err(Status::unavailable("server is shutting down"));
        }

        let executor_messages = request
            .into_inner()
            .take_while(|req| {
                let msg_ok = match req {
                    Ok(req) => req
                        .message
                        .as_ref()
                        .map(|msg| msg.executor_message_kind.is_some())
                        .unwrap_or(false),
                    Err(error) => {
                        tracing::error!(%error, "error in executor connection stream");
                        false
                    }
                };

                ready(msg_ok)
            })
            .filter_map(|req| async move {
                match req {
                    Ok(r) => r.message?.executor_message_kind,
                    Err(_) => {
                        unreachable!()
                    }
                }
            });

        let (server_messages_send, server_messages_recv) = flume::unbounded();

        self.executor_pool
            .add_executor(executor_messages, server_messages_send);

        Ok(Response::new(
            server_messages_recv
                .into_stream()
                .map(|msg| {
                    Ok(v1::ExecutorConnectionResponse {
                        message: Some(v1::ServerMessage {
                            server_message_kind: Some(msg),
                        }),
                    })
                })
                .boxed(),
        ))
    }
}
