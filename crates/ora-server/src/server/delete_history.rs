use std::{
    cmp,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ora_backend::Backend;
use wgroup::WaitGuard;

#[tracing::instrument(skip_all)]
pub(super) async fn delete_history_loop(
    backend: Arc<impl Backend>,
    history_keep_duration: Duration,
    wg: WaitGuard,
) {
    loop {
        if let Err(error) = backend
            .delete_history(
                SystemTime::now()
                    .checked_sub(history_keep_duration)
                    .unwrap_or(UNIX_EPOCH),
            )
            .await
        {
            tracing::error!(%error, "failed to delete historical data");
        }

        tokio::select! {
            _ = tokio::time::sleep(cmp::min(Duration::from_secs(60), history_keep_duration)) => {}
            _ = wg.waiting() => {
                tracing::debug!("shutting down");
                break;
            }
        }
    }
}
