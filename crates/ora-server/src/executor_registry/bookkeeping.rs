use std::time::{Duration, SystemTime};

use ora_proto::server::v1;

use crate::events::AuditEventKind;

use ora_storage::{JobTimeoutBaseTime, Storage};

use super::ServerMessage;

impl<S> super::ExecutorRegistry<S>
where
    S: Storage,
{
    pub(crate) async fn reap_dead_executors(&self) -> eyre::Result<()> {
        let mut dead_executors = Vec::new();
        let mut failed_executions = Vec::new();

        let now = SystemTime::now();

        {
            let executors = self.executors.read();

            for (executor_id, executor) in executors.iter() {
                if !executor.is_alive()
                    || executor.last_seen() <= now - self.options.executor_timeout
                {
                    dead_executors.push(*executor_id);
                    for execution_id in executor.inner.executions.read().keys() {
                        failed_executions.push(*execution_id);
                    }
                }
            }
        }

        for executor_id in dead_executors {
            if let Some(executor) = self.executors.write().swap_remove(&executor_id) {
                executor.cancel_all_executions();
                executor.disconnect();
                tracing::info!(
                    executor_id = %executor.id,
                    assigned_executions = executor.inner.executions.read().len(),
                    "executor disconnected"
                );

                if self.event_bus.audit_events_enabled() {
                    self.event_bus
                        .emit_audit_event(|| AuditEventKind::ExecutorDisconnected { executor_id });
                }
            }
        }

        if !failed_executions.is_empty() {
            self.storage
                .executions_failed(
                    &failed_executions,
                    SystemTime::now(),
                    "executor disconnected".to_string(),
                    false,
                )
                .await?;
        }

        Ok(())
    }

    pub(crate) async fn fail_timed_out_executions(&self) -> eyre::Result<()> {
        let now = SystemTime::now();
        let mut timed_out_executions = Vec::new();
        let mut timed_out_executions_unschedulable = Vec::new();

        {
            let executors = self.executors.read();

            for executor in executors.values() {
                let mut removed_executions = Vec::new();

                executor
                    .inner
                    .executions
                    .write()
                    .retain(|execution_id, state| {
                        if let Some(timeout_deadline) = state.timeout_deadline {
                            if timeout_deadline < now {
                                if state.timeout_policy.base_time
                                    == JobTimeoutBaseTime::TargetExecutionTime
                                {
                                    timed_out_executions_unschedulable.push(*execution_id);
                                } else {
                                    timed_out_executions.push(*execution_id);
                                }
                                removed_executions.push(*execution_id);

                                return false;
                            }
                        }

                        true
                    });

                if !removed_executions.is_empty() {
                    let sender = executor.inner.sender.load();

                    if let Some(sender) = &*sender {
                        for execution_id in removed_executions {
                            _ = sender.send(ServerMessage::V1(v1::ServerMessage {
                                server_message_kind: Some(
                                    v1::server_message::ServerMessageKind::ExecutionCancelled(
                                        v1::ExecutionCancelled {
                                            execution_id: execution_id.to_string(),
                                        },
                                    ),
                                ),
                            }));
                        }
                    }
                }
            }
        }

        if !timed_out_executions.is_empty() {
            self.storage
                .executions_failed(
                    &timed_out_executions,
                    SystemTime::now(),
                    "timeout".to_string(),
                    false,
                )
                .await?;
        }

        if !timed_out_executions_unschedulable.is_empty() {
            self.storage
                .executions_failed(
                    &timed_out_executions_unschedulable,
                    SystemTime::now(),
                    "timeout".to_string(),
                    true,
                )
                .await?;
        }

        Ok(())
    }

    pub(crate) async fn clean_up_orphan_executions(&self) -> eyre::Result<()> {
        let known_executors = self
            .executors
            .read()
            .values()
            .map(|e| e.id)
            .collect::<Vec<_>>();

        let orphan_execution_ids = self.storage.orphan_execution_ids(&known_executors).await?;

        if !orphan_execution_ids.is_empty() {
            self.storage
                .executions_failed(
                    &orphan_execution_ids,
                    SystemTime::now(),
                    "executor missing".to_string(),
                    false,
                )
                .await?;

            tracing::warn!(
                orphan_executions = orphan_execution_ids.len(),
                "failed executions due to missing executor"
            );
        }

        Ok(())
    }

    pub(crate) async fn shutdown(&self, executor_wait: Option<Duration>) -> eyre::Result<()> {
        // Cloning the values to avoid holding the lock while disconnecting.
        let executors = self.executors.read().values().cloned().collect::<Vec<_>>();

        for executor in executors {
            if !executor.is_alive() {
                continue;
            }

            if !executor.inner.executions.read().is_empty() {
                tracing::info!(
                    executor_id = %executor.id,
                    executions_remaining = executor.inner.executions.read().len(),
                    "executor has remaining executions"
                );

                if let Some(timeout) = executor_wait {
                    tracing::info!("waiting for executor to finish executions");
                    tokio::time::sleep(timeout).await;
                }

                if !executor.inner.executions.read().is_empty() {
                    tracing::warn!(
                        executor_id = %executor.id,
                        executions_remaining = executor.inner.executions.read().len(),
                        "disconnecting executor with remaining executions",
                    );
                }
            }

            executor.cancel_all_executions();
            executor.disconnect();
        }

        self.reap_dead_executors().await?;

        Ok(())
    }
}
