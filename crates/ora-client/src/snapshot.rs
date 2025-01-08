//! High-level client for interacting with the Ora server snapshot endpoints.

use eyre::OptionExt;
use ora_proto::snapshot::v1::{
    snapshot_service_client::SnapshotServiceClient, ImportRequest, SnapshotData,
};
use prost::bytes::Buf;
use tonic::transport::Channel;

/// A high-level client for interacting with the Ora server admin endpoints.
#[derive(Debug, Clone)]
#[must_use]
pub struct SnapshotClient {
    client: SnapshotServiceClient<Channel>,
}

impl SnapshotClient {
    /// Create a new client from a gRPC client.
    pub fn new(client: SnapshotServiceClient<Channel>) -> Self {
        Self { client }
    }

    /// Export the current state of the server to a byte buffer.
    pub async fn export_to_bytes(&self) -> eyre::Result<Vec<u8>> {
        use prost::Message;

        let mut response = self
            .client
            .clone()
            .export(tonic::Request::new(
                ora_proto::snapshot::v1::ExportRequest {},
            ))
            .await?
            .into_inner();

        let mut buf = Vec::new();

        while let Some(chunk) = response.message().await? {
            let data = chunk.data.ok_or_eyre("missing data")?;
            data.encode_length_delimited(&mut buf)?;
        }

        Ok(buf)
    }

    /// Import a snapshot into the server from a byte buffer.
    pub async fn import_from_bytes(&self, buf: Vec<u8>) -> eyre::Result<()> {
        use futures::stream::StreamExt;
        use prost::Message;

        self.client
            .clone()
            .import(tonic::Request::new(
                async_stream::stream!({
                    let mut buf = std::io::Cursor::new(buf);

                    while buf.has_remaining() {
                        let data = match SnapshotData::decode_length_delimited(&mut buf) {
                            Ok(data) => data,
                            Err(error) => {
                                tracing::error!(?error, "failed to decode snapshot data");
                                return;
                            }
                        };

                        yield ImportRequest { data: Some(data) };
                    }
                })
                .boxed(),
            ))
            .await?;

        Ok(())
    }
}
