use flume::{Receiver, Sender};
use futures::{Stream, StreamExt};

use crate::proto::executors::v1::{
    ExecutorConnectionRequest, ExecutorMessage, execution_service_client::ExecutionServiceClient,
    executor_message::ExecutorMessageKind, server_message::ServerMessageKind,
};

#[tracing::instrument(skip_all, fields(executor_id))]
pub(super) async fn connection_loop<T>(
    client: &mut ExecutionServiceClient<T>,
    incoming: Sender<ServerMessageKind>,
    outgoing: Receiver<ExecutorMessageKind>,
) where
    T: tonic::client::GrpcService<tonic::body::Body>,
    T::Error: Into<tonic::codegen::StdError>,
    T::ResponseBody:
        tonic::codegen::Body<Data = tonic::codegen::Bytes> + std::marker::Send + 'static,
    <T::ResponseBody as tonic::codegen::Body>::Error:
        Into<tonic::codegen::StdError> + std::marker::Send,
{
    let stream = client
        .executor_connection(
            hack_to_stream(outgoing).map(|msg| ExecutorConnectionRequest {
                message: Some(ExecutorMessage {
                    executor_message_kind: Some(msg),
                }),
            }),
        )
        .await;

    let mut stream = match stream {
        Ok(s) => s.into_inner(),
        Err(error) => {
            tracing::error!(%error, "failed to establish executor connection");
            return;
        }
    };

    tracing::info!("executor connected");

    loop {
        let res = match stream.message().await {
            Ok(Some(res)) => res,
            Ok(None) => {
                break;
            }
            Err(error) => {
                tracing::error!(%error, "failed to receive server message");
                break;
            }
        };

        let Some(msg) = res.message else {
            tracing::error!("received server response with no message");
            break;
        };

        let Some(msg) = msg.server_message_kind else {
            tracing::error!("received server message with no message kind");
            break;
        };

        if let ServerMessageKind::Properties(props) = &msg {
            tracing::Span::current().record("executor_id", &props.executor_id);
        }

        if incoming.send_async(msg).await.is_err() {
            break;
        }
    }

    tracing::info!("executor disconnected");
}

// Needed due to the lifetime of `RecvStream<'_, T>` not being generic enough,
// when the compiler tries to infer lifetimes.
// With this function we assert that
// it is infact a stream implementation with a `'static` lifetime.
//
// It's either a compiler bug or a limitation.
#[inline]
fn hack_to_stream<T>(recv: Receiver<T>) -> impl Stream<Item = T> + Unpin + 'static
where
    T: 'static,
{
    recv.into_stream()
}
