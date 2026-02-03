use std::time::Duration;

use flume::Sender;

use crate::proto::executors::v1::{ExecutorHeartbeat, executor_message::ExecutorMessageKind};

#[tracing::instrument(skip_all)]
pub(super) async fn heartbeat_loop(
    max_heartbeat_interval: Duration,
    server: Sender<ExecutorMessageKind>,
) {
    let target_heartbeat_interval = max_heartbeat_interval / 2;

    tracing::debug!(
        ?max_heartbeat_interval,
        ?target_heartbeat_interval,
        "sending heartbeats"
    );

    loop {
        tracing::debug!("sending heartbeat");
        if server
            .send(ExecutorMessageKind::Heartbeat(ExecutorHeartbeat {}))
            .is_err()
        {
            break;
        }

        tokio::time::sleep(target_heartbeat_interval).await;
    }
}
