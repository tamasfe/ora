use async_trait::async_trait;
use export::export_snapshot;
use futures::{StreamExt, TryStreamExt};
use ora_storage::StorageSnapshot;
use tokio::spawn;
use tracing::Instrument;

use crate::FjallStorage;

mod export;
mod import;

#[async_trait]
impl StorageSnapshot for FjallStorage {
    fn export_snapshot(
        &self,
    ) -> futures::stream::BoxStream<'static, eyre::Result<ora_proto::snapshot::v1::SnapshotData>>
    {
        let (snd, recv) = flume::unbounded();

        let this = self.clone();

        spawn(
            async move {
                let res = this
                    .read({
                        let snd = snd.clone();
                        |tx, partitions| export_snapshot(&tx, partitions, snd)
                    })
                    .await;

                if let Err(err) = res {
                    snd.send(Err(err)).unwrap();
                }
            }
            .instrument(tracing::info_span!("export_snapshot")),
        );

        recv.into_stream().boxed()
    }

    async fn import_snapshot(
        &self,
        mut snapshot: futures::stream::BoxStream<
            'static,
            eyre::Result<ora_proto::snapshot::v1::SnapshotData>,
        >,
    ) -> eyre::Result<()> {
        while let Some(data) = snapshot.try_next().await? {
            self.write(|mut tx, partitions| {
                import::import_snapshot_data(&mut tx, partitions, data)?;
                tx.commit()?;
                Ok(())
            })
            .await?;
        }

        Ok(())
    }
}
