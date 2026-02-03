use std::{
    pin::pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use futures::{Stream, StreamExt};
use ora_backend::{
    executions::{ExecutionId, ReadyExecution, StartedExecution},
    executors::ExecutorId,
    jobs::{CancelledJob, JobId, JobType, RetryPolicy},
};
use rand::seq::SliceRandom;
use tokio::{spawn, time::sleep_until};
use tonic::Status;
use uuid::Uuid;
use wgroup::{WaitGroupHandle, WaitGuard};

use crate::proto::{
    admin,
    executors::v1::{
        ExecutionCancelled, ExecutionReady, ExecutorProperties,
        executor_message::ExecutorMessageKind, server_message::ServerMessageKind,
    },
};

const MAX_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

pub(crate) enum ExecutorEvent {
    JobTypesAdded {
        job_types: Vec<JobType>,
    },
    ExecutionSucceeded {
        job_id: JobId,
        execution_id: ExecutionId,
        timestamp: SystemTime,
        output_payload_json: String,
        retry_policy: RetryPolicy,
        attempt_number: u64,
    },
    ExecutionFailed {
        job_id: JobId,
        execution_id: ExecutionId,
        timestamp: SystemTime,
        failure_reason: String,
        retry_policy: RetryPolicy,
        attempt_number: u64,
    },
    ExecutorDisconnected {
        executor: Executor,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ExecutorPool {
    /// The list of executors in the pool.
    executors: Arc<Mutex<Vec<Executor>>>,
    /// Events when certain actions happen in the executor pool.
    events: flume::Sender<ExecutorEvent>,
    wg: WaitGroupHandle,
    shutdown_grace_period: Duration,
}

impl ExecutorPool {
    pub(crate) fn new(
        events: flume::Sender<ExecutorEvent>,
        wg: WaitGroupHandle,
        shutdown_grace_period: Duration,
    ) -> Self {
        Self {
            events,
            executors: Default::default(),
            wg,
            shutdown_grace_period,
        }
    }

    pub(crate) fn add_executor(
        &self,
        executor_messages: impl Stream<Item = ExecutorMessageKind> + Send + 'static,
        server_messages: flume::Sender<ServerMessageKind>,
    ) {
        let id = ExecutorId(Uuid::new_v4());

        if server_messages
            .send(ServerMessageKind::Properties(ExecutorProperties {
                executor_id: id.0.to_string(),
                max_heartbeat_interval: Some(MAX_HEARTBEAT_INTERVAL.try_into().unwrap()),
            }))
            .is_err()
        {
            tracing::warn!("failed to initialize executor: channel closed");
            return;
        }

        let executor = Executor {
            id,
            name: None,
            job_queues: Vec::new(),
            last_heartbeat: SystemTime::now(),
            messages: server_messages,
            initialized: false,
        };
        self.executors.lock().unwrap().push(executor);

        spawn(executor_loop(
            id,
            self.executors.clone(),
            executor_messages,
            self.events.clone(),
            self.wg.add_with(&format!("executor-{id}")),
            self.shutdown_grace_period,
        ));
    }

    /// Try to schedule ready executions to available executors.
    pub(crate) fn try_assign(&self, executions: Vec<ReadyExecution>) -> Vec<StartedExecution> {
        let mut scheduled_executions = Vec::new();

        if executions.is_empty() {
            tracing::debug!("no ready executions to schedule");
            return scheduled_executions;
        }

        let mut executors = self.executors.lock().unwrap();
        let mut executors = executors.iter_mut().collect::<Vec<_>>();

        'executions_loop: for execution in executions {
            // We shuffle the executors to ensure fairness.
            executors.shuffle(&mut rand::rng());

            let now = SystemTime::now();

            for executor in &mut executors {
                let suitable_queue = executor.job_queues.iter_mut().find(|queue| {
                    queue.has_capacity()
                        && queue.job_type.id == execution.job_type_id
                        // this should normally not happen, but just in case
                        && !queue.executions.iter().any(|e| e.execution_id == execution.execution_id)
                });

                let Some(queue) = suitable_queue else {
                    continue;
                };

                let message_queued = executor
                    .messages
                    .send(ServerMessageKind::ExecutionReady(ExecutionReady {
                        job_id: execution.job_id.to_string(),
                        execution_id: execution.execution_id.to_string(),
                        job_type_id: execution.job_type_id.to_string(),
                        attempt_number: execution.attempt_number,
                        input_payload_json: execution.input_payload_json.clone(),
                        target_execution_time: Some(execution.target_execution_time.into()),
                    }))
                    .is_ok();

                if !message_queued {
                    tracing::debug!("executor outbound channel disconnected");
                    continue;
                }

                queue.add_execution(ExecutorExecution {
                    job_id: execution.job_id,
                    execution_id: execution.execution_id,
                    retry_policy: execution.retry_policy.clone(),
                    attempt_number: execution.attempt_number,
                });

                scheduled_executions.push(StartedExecution {
                    execution_id: execution.execution_id,
                    executor_id: executor.id,
                    started_at: now,
                });

                continue 'executions_loop;
            }
        }

        scheduled_executions
    }

    /// List all executors in the pool.
    pub(crate) fn list_executors(&self) -> Vec<admin::v1::Executor> {
        let executors = self.executors.lock().unwrap();

        executors
            .iter()
            .map(|executor| admin::v1::Executor {
                id: executor.id.0.to_string(),
                name: executor.name.clone(),
                last_seen_at: Some(executor.last_heartbeat.into()),
                queues: executor
                    .job_queues
                    .iter()
                    .map(|queue| admin::v1::ExecutorJobQueue {
                        job_type: Some(queue.job_type.clone().into()),
                        max_concurrent_executions: queue.max_executions,
                        active_executions: queue.executions.len() as u64,
                    })
                    .collect(),
            })
            .collect()
    }

    /// Determine whether an executor with the given ID exists in the pool.
    #[must_use]
    pub(crate) fn executor_exists(&self, executor_id: &ExecutorId) -> bool {
        let executors = self.executors.lock().unwrap();
        executors.iter().any(|e| &e.id == executor_id)
    }

    /// List all job types known to the executors.
    ///
    /// The returned vector is not sorted and may contain duplicates.
    pub(crate) fn list_job_types(&self) -> Vec<JobType> {
        let executors = self.executors.lock().unwrap();

        executors
            .iter()
            .flat_map(|executor| {
                executor
                    .job_queues
                    .iter()
                    .map(|queue| queue.job_type.clone())
            })
            .collect::<Vec<_>>()
    }

    /// Cancel in-progress executions of the given cancelled jobs.
    #[tracing::instrument(skip_all)]
    pub(crate) fn cancel_executions(&self, jobs: &[CancelledJob]) {
        let mut executors = self.executors.lock().unwrap();

        'jobs: for job in jobs {
            let execution_id = job.last_execution_id;

            for executor in &mut *executors {
                for q in &mut executor.job_queues {
                    let Some(idx) = q
                        .executions
                        .iter()
                        .position(|e| e.execution_id == execution_id)
                    else {
                        continue;
                    };

                    q.executions.swap_remove(idx);

                    _ = executor
                        .messages
                        .send(ServerMessageKind::ExecutionCancelled(ExecutionCancelled {
                            execution_id: execution_id.to_string(),
                        }));

                    continue 'jobs;
                }
            }

            tracing::debug!(%execution_id, "execution not assigned to any executors");
        }
    }
}

/// An executor in the executor pool.
#[derive(Debug)]
pub(crate) struct Executor {
    id: ExecutorId,
    name: Option<String>,
    job_queues: Vec<ExecutorJobQueue>,
    last_heartbeat: SystemTime,
    messages: flume::Sender<ServerMessageKind>,
    initialized: bool,
}

impl Executor {
    pub(crate) fn assigned_executions(&self) -> Vec<ExecutorExecution> {
        self.job_queues
            .iter()
            .flat_map(|queue| queue.executions.iter().cloned())
            .collect()
    }

    /// The count of active executions assigned to this executor.
    pub(crate) fn assigned_execution_count(&self) -> usize {
        self.job_queues
            .iter()
            .map(|queue| queue.executions.len())
            .sum()
    }
}

/// The job queue for an executor.
#[derive(Debug)]
struct ExecutorJobQueue {
    /// The job type this queue is for.
    job_type: JobType,
    /// The list of executions in the queue.
    executions: Vec<ExecutorExecution>,
    /// The maximum number of executions allowed in this queue.
    max_executions: u64,
}

impl ExecutorJobQueue {
    fn new(job_type: JobType, max_executions: u64) -> Self {
        Self {
            job_type,
            executions: Vec::new(),
            max_executions,
        }
    }

    #[inline]
    fn has_capacity(&self) -> bool {
        (self.executions.len() as u64) < self.max_executions
    }

    #[inline]
    fn add_execution(&mut self, execution_id: ExecutorExecution) {
        self.executions.push(execution_id);
    }
}

/// An execution assigned to an executor.
#[derive(Debug, Clone)]
pub(crate) struct ExecutorExecution {
    pub(crate) job_id: JobId,
    pub(crate) execution_id: ExecutionId,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) attempt_number: u64,
}

/// Run the message loop for an executor.
#[tracing::instrument(skip_all, fields(executor_id = %executor_id.0))]
async fn executor_loop(
    executor_id: ExecutorId,
    executors: Arc<Mutex<Vec<Executor>>>,
    executor_messages: impl Stream<Item = ExecutorMessageKind> + Send + 'static,
    events: flume::Sender<ExecutorEvent>,
    wg: WaitGuard,
    shutdown_grace_period: Duration,
) {
    tracing::info!("executor connected");

    let mut check_interval = tokio::time::interval(MAX_HEARTBEAT_INTERVAL);
    check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut executor_messages = pin!(executor_messages);

    let mut shutdown_deadline: Option<Instant> = None;

    loop {
        let next_tick = check_interval.tick();
        let next_message = executor_messages.next();

        let message = if let Some(shutdown_deadline) = shutdown_deadline {
            tokio::select! {
                _ = sleep_until(shutdown_deadline.into()) => {
                    let executors = executors.lock().unwrap();

                    let Some(executor) = executors.iter().find(|e| e.id == executor_id) else {
                        tracing::debug!("executor was dropped");
                        break;
                    };

                    let execution_count = executor.assigned_execution_count();

                    if execution_count > 0 {
                        for execution in executor.assigned_executions() {
                            executor
                                .messages
                                .send(ServerMessageKind::ExecutionCancelled(ExecutionCancelled {
                                    execution_id: execution.execution_id.to_string(),
                                }))
                                .ok();
                        }

                        tracing::warn!(
                            execution_count,
                            executor_id = %executor_id.0,
                            "shutdown grace period elapsed, cancelled executions",
                        );
                    }

                    break;
                }
                message = next_message => {
                    match message {
                        Some(message) => message,
                        None => {
                            tracing::debug!("executor message channel closed");
                            break;
                        }
                    }
                }
            }
        } else {
            tokio::select! {
                _ = wg.waiting() => {
                    if shutdown_grace_period.is_zero() {
                        tracing::debug!(
                            executor_id = %executor_id.0,
                            "immediate shutdown requested, disconnecting executor",
                        );
                        break;
                    }

                    let executors = executors.lock().unwrap();
                    let Some(executor) = executors.iter().find(|e| e.id == executor_id) else {
                        tracing::debug!("executor was dropped");
                        break;
                    };

                    if executor.assigned_execution_count() > 0 {
                        tracing::info!(
                            executor_id = %executor_id.0,
                            grace_period = ?shutdown_grace_period,
                            "waiting for executor to finish executions before shutdown",
                        );
                        shutdown_deadline = Some(Instant::now() + shutdown_grace_period);
                        continue;
                    }

                    break;
                }
                _ = next_tick => {
                    let mut executors = executors.lock().unwrap();
                    let Some(executor) = executors.iter().find(|e| e.id == executor_id) else {
                        tracing::debug!("executor was dropped");
                        break;
                    };

                    if SystemTime::now().duration_since(executor.last_heartbeat).unwrap_or_default() > MAX_HEARTBEAT_INTERVAL {
                        tracing::warn!("executor heartbeat timeout, disconnecting");
                        drop_executor(executor_id, &mut executors, &events);
                        break;
                    }

                    if executor.messages.is_disconnected() {
                        tracing::debug!("executor outbound channel disconnected");
                        drop_executor(executor_id, &mut executors, &events);
                        break;
                    }

                    continue;
                }
                message = next_message => {
                    match message {
                        Some(message) => message,
                        None => {
                            tracing::debug!("executor message channel closed");
                            break;
                        }
                    }
                }
            }
        };

        let mut executors = executors.lock().unwrap();

        let Some(executor) = executors.iter_mut().find(|e| e.id == executor_id) else {
            tracing::debug!("executor was dropped");
            break;
        };

        if executor.messages.is_disconnected() {
            tracing::debug!("executor outbound channel disconnected");
            drop_executor(executor_id, &mut executors, &events);
            break;
        }

        match message {
            ExecutorMessageKind::Capabilities(executor_capabilities) => {
                if executor.initialized {
                    tracing::error!("executor sent duplicate capabilities message, disconnecting");
                    drop_executor(executor_id, &mut executors, &events);
                    break;
                }

                executor.name = Some(executor_capabilities.name);

                let job_queues = executor_capabilities
                    .job_queues
                    .into_iter()
                    .map(|q| {
                        Ok(ExecutorJobQueue::new(
                            q.job_type
                                .ok_or_else(|| {
                                    Status::invalid_argument("missing job type for job queue")
                                })?
                                .try_into()?,
                            q.max_concurrent_executions,
                        ))
                    })
                    .collect::<Result<Vec<_>, Status>>();

                let job_type_count = match job_queues {
                    Ok(job_queues) => {
                        let mut new_job_types = job_queues
                            .iter()
                            .map(|q| q.job_type.clone())
                            .collect::<Vec<_>>();

                        new_job_types.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
                        new_job_types.dedup_by(|a, b| a.id.as_str().eq(b.id.as_str()));

                        if !new_job_types.is_empty()
                            && events
                                .send(ExecutorEvent::JobTypesAdded {
                                    job_types: new_job_types,
                                })
                                .is_err()
                        {
                            tracing::debug!("internal event channel closed");
                            break;
                        }

                        executor.job_queues = job_queues;
                        executor.job_queues.len()
                    }
                    Err(error) => {
                        let msg = error.message();
                        tracing::error!("executor sent invalid capabilities message: {msg}");
                        drop_executor(executor_id, &mut executors, &events);
                        break;
                    }
                };

                executor.initialized = true;
                tracing::info!(
                    executor_name = executor.name.as_deref().unwrap_or(""),
                    job_type_count,
                    "executor initialized",
                );
            }
            ExecutorMessageKind::Heartbeat(_) => {
                executor.last_heartbeat = SystemTime::now();
            }
            ExecutorMessageKind::ExecutionSucceeded(execution_succeeded) => {
                let Ok(execution_id) = execution_succeeded.execution_id.parse::<Uuid>() else {
                    tracing::error!("invalid execution ID");
                    break;
                };

                let execution_id = ExecutionId(execution_id);

                let Some(execution) = executor.job_queues.iter_mut().find_map(|q| {
                    let idx = q
                        .executions
                        .iter()
                        .position(|e| e.execution_id == execution_id)?;

                    Some(q.executions.swap_remove(idx))
                }) else {
                    tracing::error!("executor completed execution that was not assigned to it");
                    drop_executor(executor_id, &mut executors, &events);
                    break;
                };

                let timestamp = match execution_succeeded.timestamp {
                    Some(ts) => match SystemTime::try_from(ts) {
                        Ok(ts) => ts,
                        Err(error) => {
                            tracing::error!("invalid execution success timestamp: {error}");
                            break;
                        }
                    },
                    None => SystemTime::now(),
                };

                if events
                    .send(ExecutorEvent::ExecutionSucceeded {
                        job_id: execution.job_id,
                        execution_id,
                        timestamp,
                        output_payload_json: execution_succeeded.output_payload_json,
                        retry_policy: execution.retry_policy,
                        attempt_number: execution.attempt_number,
                    })
                    .is_err()
                {
                    tracing::debug!("internal event channel closed");
                    break;
                }
            }
            ExecutorMessageKind::ExecutionFailed(execution_failed) => {
                let Ok(execution_id) = execution_failed.execution_id.parse::<Uuid>() else {
                    tracing::error!("invalid execution ID");
                    drop_executor(executor_id, &mut executors, &events);
                    break;
                };

                let execution_id = ExecutionId(execution_id);

                let Some(execution) = executor.job_queues.iter_mut().find_map(|q| {
                    let idx = q
                        .executions
                        .iter()
                        .position(|e| e.execution_id == execution_id)?;

                    Some(q.executions.swap_remove(idx))
                }) else {
                    tracing::error!("executor completed execution that was not assigned to it");
                    drop_executor(executor_id, &mut executors, &events);
                    break;
                };

                let timestamp = match execution_failed.timestamp {
                    Some(ts) => match SystemTime::try_from(ts) {
                        Ok(ts) => ts,
                        Err(error) => {
                            tracing::error!("invalid execution failure timestamp: {error}");
                            break;
                        }
                    },
                    None => SystemTime::now(),
                };

                if events
                    .send(ExecutorEvent::ExecutionFailed {
                        job_id: execution.job_id,
                        execution_id,
                        timestamp,
                        failure_reason: execution_failed.failure_reason,
                        retry_policy: execution.retry_policy,
                        attempt_number: execution.attempt_number,
                    })
                    .is_err()
                {
                    tracing::debug!("internal event channel closed");
                    break;
                }
            }
        }
    }

    tracing::debug!("executor message channel closed");
    // Remove the executor from the pool.
    let mut executors = executors.lock().unwrap();
    drop_executor(executor_id, &mut executors, &events);
    tracing::info!("executor disconnected");
}

fn drop_executor(
    executor_id: ExecutorId,
    executors: &mut Vec<Executor>,
    events: &flume::Sender<ExecutorEvent>,
) {
    let Some(position) = executors.iter().position(|e| e.id == executor_id) else {
        return;
    };

    let executor = executors.swap_remove(position);
    let _ = events.send(ExecutorEvent::ExecutorDisconnected { executor });
    tracing::debug!(%executor_id, "executor dropped from pool");
}
