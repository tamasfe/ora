use std::time::SystemTime;

use ora_proto::server::v1;
use uuid::Uuid;

use crate::executor_registry::handle::ExecutionState;
use ora_storage::{JobTimeoutBaseTime, ReadyExecution, Storage};

use super::{ExecutorHandle, ServerMessage};

impl<S> super::ExecutorRegistry<S>
where
    S: Storage,
{
    /// Assign any unassigned ready executions to available executors.
    pub(crate) async fn assign_executions(&self) -> eyre::Result<()> {
        let mut total_execution_count = 0_usize;
        let mut assigned_execution_count = 0_usize;
        let mut last_id = None;

        loop {
            let executions = self.storage.ready_executions(last_id).await?;

            if executions.is_empty() {
                break;
            }

            for execution in executions {
                total_execution_count += 1;
                last_id = Some(execution.id);

                let candidates = self.collect_candidates(&execution);

                if candidates.is_empty() {
                    tracing::debug!(execution_id = %execution.id, "no executors available");
                    continue;
                }

                // Find an executor with the fewest number of executions.
                let executor = candidates
                    .iter()
                    .min_by_key(|e| e.inner.executions.read().len())
                    .unwrap();

                let timeout_deadline = if let Some(timeout) = execution.timeout_policy.timeout {
                    if execution.timeout_policy.base_time == JobTimeoutBaseTime::TargetExecutionTime
                    {
                        Some(execution.target_execution_time + timeout)
                    } else {
                        None
                    }
                } else {
                    None
                };

                let sender = executor.inner.sender.load();

                if let Some(sender) = &*sender {
                    _ = sender.send(ServerMessage::V1(v1::ServerMessage {
                        server_message_kind: Some(
                            v1::server_message::ServerMessageKind::ExecutionReady(
                                v1::ExecutionReady {
                                    attempt_number: execution.attempt_number,
                                    execution_id: execution.id.to_string(),
                                    input_payload_json: execution.input_payload_json,
                                    job_id: execution.job_id.to_string(),
                                    target_execution_time: Some(
                                        execution.target_execution_time.into(),
                                    ),
                                    job_type_id: execution.job_type_id,
                                },
                            ),
                        ),
                    }));

                    executor.inner.executions.write().insert(
                        execution.id,
                        ExecutionState {
                            timeout_policy: execution.timeout_policy,
                            timeout_deadline,
                            started_at: None,
                        },
                    );

                    self.storage
                        .execution_assigned(execution.id, executor.id, SystemTime::now())
                        .await?;

                    tracing::debug!(
                        execution_id = %execution.id,
                        executor_id = %executor.id,
                        "execution assigned",
                    );

                    assigned_execution_count += 1;
                } else {
                    tracing::debug!(
                        execution_id = %execution.id,
                        executor_id = %executor.id,
                        "executor disconnected",
                    );
                }
            }
        }

        tracing::debug!(
            total_execution_count = total_execution_count,
            assigned_execution_count = assigned_execution_count,
            "executions assigned",
        );

        Ok(())
    }

    /// Collect a list of candidate executors for assignment.
    fn collect_candidates(&self, execution: &ReadyExecution) -> Vec<ExecutorHandle> {
        let mut candidates = Vec::new();
        let executors = self.executors.read();

        for executor in executors.values() {
            if !executor.is_ready() {
                continue;
            }

            let capabilities = executor.inner.capabilities.load();

            let supports_job_type = capabilities
                .as_ref()
                .map(|c| c.job_types.contains(&execution.job_type_id))
                .unwrap_or(false);

            let has_capacity = capabilities
                .as_ref()
                .and_then(|c| c.max_concurrent_executions)
                .map(|max| {
                    let executions = executor.inner.executions.read();
                    executions.len() < usize::try_from(max.get()).unwrap()
                })
                .unwrap_or(true);

            if supports_job_type && has_capacity {
                candidates.push(executor.clone());
            }
        }

        candidates
    }

    /// Cancel executions.
    pub(crate) fn cancel_executions(&self, execution_ids: &[Uuid]) {
        for execution_id in execution_ids {
            let mut executors = self.executors.write();

            for executor in executors.values_mut() {
                let should_send_cancellation = executor
                    .inner
                    .executions
                    .write()
                    .remove(execution_id)
                    .is_some();

                if should_send_cancellation {
                    let sender = executor.inner.sender.load();

                    if let Some(sender) = &*sender {
                        _ = sender
                            .send(ServerMessage::V1(v1::ServerMessage {
                                server_message_kind: Some(
                                    v1::server_message::ServerMessageKind::ExecutionCancelled(
                                        v1::ExecutionCancelled {
                                            execution_id: execution_id.to_string(),
                                        },
                                    ),
                                ),
                            }))
                            .map_err(|_| eyre::eyre!("failed to queue executor message"));
                    }
                    break;
                }
            }
        }
    }
}
