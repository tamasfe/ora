use std::{
    collections::{HashMap, HashSet},
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
use tokio::{
    spawn,
    sync::{Notify, futures::Notified},
    time::sleep_until,
};
use tonic::Status;
use uuid::Uuid;
use wgroup::{WaitGroupHandle, WaitGuard};

use crate::{
    proto::{
        admin,
        executors::v1::{
            ExecutionCancelled, ExecutionReady, ExecutorProperties,
            executor_message::ExecutorMessageKind, server_message::ServerMessageKind,
        },
    },
    util::deadline_after,
};

const MAX_HEARTBEAT_INTERVAL: Duration = Duration::from_mins(1);

/// The time executors have to accept or reject an offered execution,
/// after which the offer is withdrawn.
const OFFER_TIMEOUT: Duration = Duration::from_secs(30);

/// The interval of checking for expired offers.
const OFFER_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// The time no executions are offered to a job queue
/// of an executor after it rejected an execution.
///
/// The backoff is doubled for every consecutive rejection.
const MIN_REJECTION_BACKOFF: Duration = Duration::from_secs(1);

/// The maximum time no executions are offered to a job queue
/// of an executor after consecutive rejections.
const MAX_REJECTION_BACKOFF: Duration = Duration::from_secs(30);

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
    /// Executions accepted by executors that should be started in the backend.
    ///
    /// These are separate from the other events so that starting executions
    /// is not delayed by processing execution results.
    accepted: flume::Sender<StartedExecution>,
    /// Notified when accepted executions were started in the backend
    /// (or could not be started).
    starts_recorded: Arc<Notify>,
    /// Notified when executors might be able to accept
    /// executions they could not before.
    executor_available: Arc<Notify>,
    /// Accepted executions that must not be offered again.
    accepted_executions: Arc<Mutex<AcceptedExecutions>>,
    wg: WaitGroupHandle,
    shutdown_grace_period: Duration,
}

/// The result of offering ready executions to executors.
#[derive(Debug, Default)]
pub(crate) struct OfferedExecutions {
    /// The number of executions offered to executors.
    pub(crate) offered_count: usize,
    /// The IDs of executions that were not offered,
    /// either because no executor could accept them,
    /// or because they are already offered to or accepted by an executor.
    pub(crate) not_offered: Vec<ExecutionId>,
}

/// Executions accepted by executors that might still
/// be returned as ready by the backend.
///
/// Accepted executions are only started in the backend after some delay,
/// and ready executions fetched earlier might be stale, these executions
/// must not be offered again even if they are not in the pool anymore
/// (e.g. because they finished in the meantime).
#[derive(Debug, Default)]
struct AcceptedExecutions {
    /// The accepted executions and the time when they were started
    /// in the backend (if they were).
    executions: HashMap<ExecutionId, Option<Instant>>,
}

impl ExecutorPool {
    pub(crate) fn new(
        events: flume::Sender<ExecutorEvent>,
        accepted: flume::Sender<StartedExecution>,
        wg: WaitGroupHandle,
        shutdown_grace_period: Duration,
    ) -> Self {
        Self {
            events,
            accepted,
            starts_recorded: Default::default(),
            executors: Default::default(),
            executor_available: Default::default(),
            accepted_executions: Default::default(),
            wg,
            shutdown_grace_period,
        }
    }

    /// Wait until executors might be able to accept more executions,
    /// either because executions finished or new executors connected.
    ///
    /// Only changes after this function is called are observed.
    pub(crate) fn executor_available(&self) -> Notified<'_> {
        self.executor_available.notified()
    }

    /// Whether any executor can accept an execution of any job type.
    #[must_use]
    pub(crate) fn has_capacity(&self) -> bool {
        let executors = self.executors.lock().unwrap();
        let now = Instant::now();
        executors
            .iter()
            .flat_map(|executor| &executor.job_queues)
            .any(|queue| queue.has_capacity(now))
    }

    /// The earliest time when a job queue of an executor
    /// that rejected executions can be offered executions again.
    #[must_use]
    pub(crate) fn earliest_backoff_end(&self) -> Option<Instant> {
        let executors = self.executors.lock().unwrap();
        let now = Instant::now();
        executors
            .iter()
            .flat_map(|executor| &executor.job_queues)
            .filter_map(|queue| queue.backoff_until)
            .filter(|until| *until > now)
            .min()
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
            last_heartbeat_at: Instant::now(),
            messages: server_messages,
            initialized: false,
            execution_handshake: false,
        };
        self.executors.lock().unwrap().push(executor);

        spawn(executor_loop(
            id,
            self.executors.clone(),
            executor_messages,
            self.events.clone(),
            self.accepted.clone(),
            self.executor_available.clone(),
            self.accepted_executions.clone(),
            self.wg.add_with(&format!("executor-{id}")),
            self.shutdown_grace_period,
        ));
    }

    /// Try to offer ready executions to available executors.
    ///
    /// The executions are assigned to the executors only once they accept them,
    /// which is reported by [`ExecutorEvent::ExecutionAccepted`] events.
    ///
    /// The `fetched_at` time must be taken before the ready executions
    /// were fetched from the backend, executions that were started in the backend
    /// since are not offered again.
    pub(crate) fn try_offer(
        &self,
        executions: Vec<ReadyExecution>,
        fetched_at: Instant,
    ) -> OfferedExecutions {
        let mut offered = OfferedExecutions::default();

        if executions.is_empty() {
            tracing::debug!("no ready executions to offer");
            return offered;
        }

        let mut executors = self.executors.lock().unwrap();
        let mut accepted_executions = self.accepted_executions.lock().unwrap();

        // Executions started before the ready executions were fetched
        // are not returned by the backend anymore.
        accepted_executions
            .executions
            .retain(|_, started_at| started_at.is_none_or(|started_at| started_at >= fetched_at));

        let held_execution_ids = executors
            .iter()
            .flat_map(|executor| &executor.job_queues)
            .flat_map(|queue| &queue.executions)
            .map(|execution| execution.execution_id)
            .chain(accepted_executions.executions.keys().copied())
            .collect::<HashSet<_>>();

        let mut executors = executors.iter_mut().collect::<Vec<_>>();

        let now = Instant::now();

        'executions_loop: for execution in executions {
            if held_execution_ids.contains(&execution.execution_id) {
                offered.not_offered.push(execution.execution_id);
                continue;
            }

            // We shuffle the executors to ensure fairness.
            executors.shuffle(&mut rand::rng());

            for executor in &mut executors {
                let suitable_queue = executor.job_queues.iter_mut().find(|queue| {
                    queue.has_capacity(now) && queue.job_type.id == execution.job_type_id
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

                // Executors without the execution handshake
                // accept every execution sent to them.
                let state = if executor.execution_handshake {
                    ExecutionState::Offered {
                        deadline: now + OFFER_TIMEOUT,
                        offered_at: SystemTime::now(),
                    }
                } else {
                    accepted_executions
                        .executions
                        .insert(execution.execution_id, None);

                    if self
                        .accepted
                        .send(StartedExecution {
                            execution_id: execution.execution_id,
                            executor_id: executor.id,
                            started_at: SystemTime::now(),
                        })
                        .is_err()
                    {
                        tracing::debug!("internal accepted executions channel closed");
                    }

                    ExecutionState::Accepted
                };

                queue.add_execution(ExecutorExecution {
                    job_id: execution.job_id,
                    execution_id: execution.execution_id,
                    retry_policy: execution.retry_policy.clone(),
                    attempt_number: execution.attempt_number,
                    state,
                });

                offered.offered_count += 1;

                continue 'executions_loop;
            }

            offered.not_offered.push(execution.execution_id);
        }

        offered
    }

    /// The IDs of executions that are offered to executors
    /// or were accepted but not yet started in the backend.
    ///
    /// These executions are still pending in the backend,
    /// but must not be offered again.
    pub(crate) fn in_flight_execution_ids(&self) -> Vec<ExecutionId> {
        let executors = self.executors.lock().unwrap();
        let accepted_executions = self.accepted_executions.lock().unwrap();

        executors
            .iter()
            .flat_map(|executor| &executor.job_queues)
            .flat_map(|queue| &queue.executions)
            .filter(|execution| !execution.is_accepted())
            .map(|execution| execution.execution_id)
            .chain(
                accepted_executions
                    .executions
                    .iter()
                    .filter(|(_, started_at)| started_at.is_none())
                    .map(|(execution_id, _)| *execution_id),
            )
            .collect()
    }

    /// Mark accepted executions as started in the backend.
    pub(crate) fn accepted_executions_started(&self, execution_ids: &[ExecutionId]) {
        {
            let mut accepted_executions = self.accepted_executions.lock().unwrap();
            let now = Instant::now();

            for execution_id in execution_ids {
                if let Some(started_at) = accepted_executions.executions.get_mut(execution_id) {
                    *started_at = Some(now);
                }
            }
        }

        self.starts_recorded.notify_waiters();
    }

    /// Forget accepted executions that could not be started in the backend,
    /// so that they can be offered again.
    pub(crate) fn accepted_executions_not_started(&self, execution_ids: &[ExecutionId]) {
        {
            let mut accepted_executions = self.accepted_executions.lock().unwrap();

            for execution_id in execution_ids {
                accepted_executions.executions.remove(execution_id);
            }
        }

        self.starts_recorded.notify_waiters();
    }

    /// Wait until the given executions are not waiting to be started
    /// in the backend anymore, but at most for the given duration.
    ///
    /// Returns `false` if the timeout elapsed.
    pub(crate) async fn wait_for_starts(
        &self,
        execution_ids: &[ExecutionId],
        timeout: Duration,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            // Created before checking so that no notification is missed.
            let starts_recorded = self.starts_recorded.notified();

            let start_pending = {
                let accepted_executions = self.accepted_executions.lock().unwrap();
                execution_ids.iter().any(|execution_id| {
                    matches!(accepted_executions.executions.get(execution_id), Some(None))
                })
            };

            if !start_pending {
                return true;
            }

            if tokio::time::timeout_at(deadline, starts_recorded)
                .await
                .is_err()
            {
                return false;
            }
        }
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

    /// Whether there are no executors in the pool.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.executors.lock().unwrap().is_empty()
    }

    /// The grace period executors are given on shutdown.
    #[must_use]
    pub(crate) fn shutdown_grace_period(&self) -> Duration {
        self.shutdown_grace_period
    }

    /// Wait until there are no executors in the pool,
    /// but at most for the given duration.
    pub(crate) async fn wait_empty(&self, timeout: Duration) {
        let deadline = deadline_after(timeout);

        while !self.is_empty() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Cancel in-progress executions of the given cancelled jobs.
    pub(crate) fn cancel_executions(&self, jobs: &[CancelledJob]) {
        self.cancel_execution_ids(
            &jobs
                .iter()
                .map(|job| job.last_execution_id)
                .collect::<Vec<_>>(),
        );
    }

    /// Cancel the given executions on the executors they are assigned to,
    /// freeing up their capacity.
    #[tracing::instrument(skip_all)]
    pub(crate) fn cancel_execution_ids(&self, execution_ids: &[ExecutionId]) {
        let mut executors = self.executors.lock().unwrap();
        let mut cancelled_any = false;

        'executions: for &execution_id in execution_ids {
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
                    cancelled_any = true;

                    _ = executor
                        .messages
                        .send(ServerMessageKind::ExecutionCancelled(ExecutionCancelled {
                            execution_id: execution_id.to_string(),
                        }));

                    continue 'executions;
                }
            }

            tracing::debug!(%execution_id, "execution not assigned to any executors");
        }

        if cancelled_any {
            self.executor_available.notify_waiters();
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
    /// The time of the last heartbeat for detecting timeouts,
    /// unaffected by changes of the system clock.
    last_heartbeat_at: Instant,
    messages: flume::Sender<ServerMessageKind>,
    initialized: bool,
    /// Whether the executor accepts or rejects offered executions
    /// before they are assigned to it.
    execution_handshake: bool,
}

impl Executor {
    /// The executions accepted by this executor.
    pub(crate) fn assigned_executions(&self) -> Vec<ExecutorExecution> {
        self.job_queues
            .iter()
            .flat_map(|queue| queue.executions.iter())
            .filter(|execution| execution.is_accepted())
            .cloned()
            .collect()
    }

    /// The count of executions accepted by this executor.
    pub(crate) fn assigned_execution_count(&self) -> usize {
        self.job_queues
            .iter()
            .flat_map(|queue| queue.executions.iter())
            .filter(|execution| execution.is_accepted())
            .count()
    }

    /// Find an execution offered to or accepted by this executor.
    fn find_execution_mut(
        &mut self,
        execution_id: ExecutionId,
    ) -> Option<(&mut ExecutorJobQueue, usize)> {
        self.job_queues.iter_mut().find_map(|queue| {
            let idx = queue
                .executions
                .iter()
                .position(|e| e.execution_id == execution_id)?;
            Some((queue, idx))
        })
    }

    /// Remove an execution offered to or accepted by this executor.
    fn remove_execution(&mut self, execution_id: ExecutionId) -> Option<ExecutorExecution> {
        let (queue, idx) = self.find_execution_mut(execution_id)?;
        Some(queue.executions.swap_remove(idx))
    }

    /// Withdraw offered executions from the executor.
    ///
    /// If `expired_at` is given, only offers that expired by then are
    /// withdrawn, and they count as rejections.
    ///
    /// Returns the number of withdrawn offers.
    fn withdraw_offers(&mut self, expired_at: Option<Instant>) -> usize {
        let mut withdrawn_count = 0;

        for queue in &mut self.job_queues {
            let mut queue_withdrawn_count = 0;

            queue.executions.retain(|execution| {
                let ExecutionState::Offered { deadline, .. } = execution.state else {
                    return true;
                };

                if expired_at.is_some_and(|expired_at| deadline > expired_at) {
                    return true;
                }

                _ = self
                    .messages
                    .send(ServerMessageKind::ExecutionCancelled(ExecutionCancelled {
                        execution_id: execution.execution_id.to_string(),
                    }));

                queue_withdrawn_count += 1;
                false
            });

            if queue_withdrawn_count > 0 && expired_at.is_some() {
                queue.rejected();
            }

            withdrawn_count += queue_withdrawn_count;
        }

        withdrawn_count
    }
}

/// The job queue for an executor.
#[derive(Debug)]
struct ExecutorJobQueue {
    /// The job type this queue is for.
    job_type: JobType,
    /// The list of executions offered to or accepted by the executor.
    executions: Vec<ExecutorExecution>,
    /// The maximum number of executions allowed in this queue.
    max_executions: u64,
    /// No executions are offered until this time
    /// because the executor rejected executions.
    backoff_until: Option<Instant>,
    /// The number of consecutive rejections.
    consecutive_rejections: u32,
}

impl ExecutorJobQueue {
    fn new(job_type: JobType, max_executions: u64) -> Self {
        Self {
            job_type,
            executions: Vec::new(),
            max_executions,
            backoff_until: None,
            consecutive_rejections: 0,
        }
    }

    #[inline]
    fn has_capacity(&self, now: Instant) -> bool {
        (self.executions.len() as u64) < self.max_executions
            && self.backoff_until.is_none_or(|until| until <= now)
    }

    #[inline]
    fn add_execution(&mut self, execution_id: ExecutorExecution) {
        self.executions.push(execution_id);
    }

    /// The executor accepted an execution of this queue.
    fn accepted(&mut self) {
        self.consecutive_rejections = 0;
        self.backoff_until = None;
    }

    /// The executor rejected an execution of this queue,
    /// so no executions are offered for a while.
    fn rejected(&mut self) {
        let backoff = MIN_REJECTION_BACKOFF
            .saturating_mul(2u32.saturating_pow(self.consecutive_rejections))
            .min(MAX_REJECTION_BACKOFF);

        self.consecutive_rejections = self.consecutive_rejections.saturating_add(1);
        self.backoff_until = Some(Instant::now() + backoff);
    }
}

/// An execution offered to or accepted by an executor.
#[derive(Debug, Clone)]
pub(crate) struct ExecutorExecution {
    pub(crate) job_id: JobId,
    pub(crate) execution_id: ExecutionId,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) attempt_number: u64,
    state: ExecutionState,
}

impl ExecutorExecution {
    fn is_accepted(&self) -> bool {
        matches!(self.state, ExecutionState::Accepted)
    }
}

/// The state of an execution in the executor pool.
#[derive(Debug, Clone, Copy)]
enum ExecutionState {
    /// The execution was offered to the executor,
    /// which must accept or reject it until the deadline.
    Offered {
        deadline: Instant,
        /// The time the execution was offered,
        /// it is considered the start time of the execution if accepted.
        offered_at: SystemTime,
    },
    /// The execution was accepted by the executor.
    Accepted,
}

/// Run the message loop for an executor.
#[tracing::instrument(skip_all, fields(executor_id = %executor_id.0))]
async fn executor_loop(
    executor_id: ExecutorId,
    executors: Arc<Mutex<Vec<Executor>>>,
    executor_messages: impl Stream<Item = ExecutorMessageKind> + Send + 'static,
    events: flume::Sender<ExecutorEvent>,
    accepted: flume::Sender<StartedExecution>,
    executor_available: Arc<Notify>,
    accepted_executions: Arc<Mutex<AcceptedExecutions>>,
    wg: WaitGuard,
    shutdown_grace_period: Duration,
) {
    tracing::info!("executor connected");

    let mut check_interval = tokio::time::interval(MAX_HEARTBEAT_INTERVAL);
    check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut offer_check_interval = tokio::time::interval(OFFER_CHECK_INTERVAL);
    offer_check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut executor_messages = pin!(executor_messages);

    let mut shutdown_deadline: Option<Instant> = None;

    loop {
        let next_tick = check_interval.tick();
        let next_offer_check = offer_check_interval.tick();
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

                    let mut executors = executors.lock().unwrap();
                    let Some(executor) = executors.iter_mut().find(|e| e.id == executor_id) else {
                        tracing::debug!("executor was dropped");
                        break;
                    };

                    // No new executions are accepted during shutdown.
                    let withdrawn_count = executor.withdraw_offers(None);

                    if withdrawn_count > 0 {
                        tracing::debug!(withdrawn_count, "withdrew offered executions before shutdown");
                    }

                    if executor.assigned_execution_count() > 0 {
                        tracing::info!(
                            executor_id = %executor_id.0,
                            grace_period = ?shutdown_grace_period,
                            "waiting for executor to finish executions before shutdown",
                        );
                        shutdown_deadline = Some(deadline_after(shutdown_grace_period));
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

                    if executor.last_heartbeat_at.elapsed() > MAX_HEARTBEAT_INTERVAL {
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
                _ = next_offer_check => {
                    let mut executors = executors.lock().unwrap();
                    let Some(executor) = executors.iter_mut().find(|e| e.id == executor_id) else {
                        tracing::debug!("executor was dropped");
                        break;
                    };

                    let expired_count = executor.withdraw_offers(Some(Instant::now()));

                    if expired_count > 0 {
                        tracing::warn!(
                            expired_count,
                            "executor did not respond to offered executions in time, withdrew them"
                        );
                        executor_available.notify_waiters();
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
                executor.execution_handshake = executor_capabilities.execution_handshake;

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
                executor_available.notify_waiters();
                tracing::info!(
                    executor_name = executor.name.as_deref().unwrap_or(""),
                    job_type_count,
                    "executor initialized",
                );
            }
            ExecutorMessageKind::Heartbeat(_) => {
                executor.last_heartbeat = SystemTime::now();
                executor.last_heartbeat_at = Instant::now();
            }
            ExecutorMessageKind::ExecutionAccepted(execution_accepted) => {
                let Ok(execution_id) = execution_accepted.execution_id.parse::<Uuid>() else {
                    tracing::error!("invalid execution ID");
                    drop_executor(executor_id, &mut executors, &events);
                    break;
                };

                let execution_id = ExecutionId(execution_id);

                let cancel = |executor: &Executor| {
                    _ = executor
                        .messages
                        .send(ServerMessageKind::ExecutionCancelled(ExecutionCancelled {
                            execution_id: execution_id.to_string(),
                        }));
                };

                let Some((queue, idx)) = executor.find_execution_mut(execution_id) else {
                    // The offer was withdrawn (e.g. it expired or the job was cancelled),
                    // the executor must not run it.
                    tracing::debug!(%execution_id, "executor accepted an execution that was not offered to it");
                    cancel(executor);
                    continue;
                };

                let ExecutionState::Offered { offered_at, .. } = queue.executions[idx].state else {
                    tracing::warn!(%execution_id, "executor accepted an execution multiple times");
                    continue;
                };

                // The start time is the time when the execution was sent to the executor
                // by the server (like before executions were explicitly accepted),
                // as timeouts are measured by the server, and the executor
                // might report results with timestamps before the acceptance is received.
                let started_at = offered_at;

                if shutdown_deadline.is_some() {
                    tracing::debug!(%execution_id, "executor accepted an execution during shutdown");
                    queue.executions.swap_remove(idx);
                    cancel(executor);
                    continue;
                }

                queue.executions[idx].state = ExecutionState::Accepted;
                queue.accepted();

                accepted_executions
                    .lock()
                    .unwrap()
                    .executions
                    .insert(execution_id, None);

                if accepted
                    .send(StartedExecution {
                        execution_id,
                        executor_id,
                        started_at,
                    })
                    .is_err()
                {
                    tracing::debug!("internal accepted executions channel closed");
                    break;
                }
            }
            ExecutorMessageKind::ExecutionRejected(execution_rejected) => {
                let Ok(execution_id) = execution_rejected.execution_id.parse::<Uuid>() else {
                    tracing::error!("invalid execution ID");
                    drop_executor(executor_id, &mut executors, &events);
                    break;
                };

                let execution_id = ExecutionId(execution_id);

                let Some((queue, idx)) = executor.find_execution_mut(execution_id) else {
                    tracing::debug!(%execution_id, "executor rejected an execution that was not offered to it");
                    continue;
                };

                if queue.executions[idx].is_accepted() {
                    tracing::warn!(%execution_id, "executor rejected an execution it already accepted, ignoring");
                    continue;
                }

                // The execution is still pending in the backend,
                // it will be offered again later.
                queue.executions.swap_remove(idx);
                queue.rejected();
                executor_available.notify_waiters();

                tracing::debug!(
                    %execution_id,
                    reason = execution_rejected.reason.as_deref().unwrap_or_default(),
                    "executor rejected execution"
                );
            }
            ExecutorMessageKind::ExecutionSucceeded(execution_succeeded) => {
                let Ok(execution_id) = execution_succeeded.execution_id.parse::<Uuid>() else {
                    tracing::error!("invalid execution ID");
                    break;
                };

                let execution_id = ExecutionId(execution_id);

                let Some(execution) = executor.remove_execution(execution_id) else {
                    // this can happen when an execution gets cancelled
                    tracing::debug!("executor completed execution that was not assigned to it");
                    continue;
                };

                executor_available.notify_waiters();

                if !execution.is_accepted() {
                    tracing::warn!(
                        %execution_id,
                        "executor completed an execution without accepting it"
                    );
                    continue;
                }

                let timestamp = match execution_succeeded.timestamp {
                    // Timestamps in the future (e.g. due to clock skew)
                    // might not be representable by the backend.
                    Some(ts) => match SystemTime::try_from(ts) {
                        Ok(ts) => ts.min(SystemTime::now()),
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

                let Some(execution) = executor.remove_execution(execution_id) else {
                    // this can happen when an execution gets cancelled
                    tracing::debug!("executor completed execution that was not assigned to it");
                    continue;
                };

                executor_available.notify_waiters();

                if !execution.is_accepted() {
                    tracing::warn!(
                        %execution_id,
                        "executor completed an execution without accepting it"
                    );
                    continue;
                }

                let timestamp = match execution_failed.timestamp {
                    // Timestamps in the future (e.g. due to clock skew)
                    // might not be representable by the backend.
                    Some(ts) => match SystemTime::try_from(ts) {
                        Ok(ts) => ts.min(SystemTime::now()),
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

        // There is no need to wait for the rest of the grace period.
        if shutdown_deadline.is_some() && executor.assigned_execution_count() == 0 {
            tracing::info!("executor finished its executions before shutdown");
            break;
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
