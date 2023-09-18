//! Worker implementation.

use std::{
    collections::HashMap, iter::once, mem, num::NonZeroUsize, pin::pin, sync::Arc, time::Duration,
};

use futures::{stream::FuturesUnordered, StreamExt, TryStreamExt};
use ora_common::task::WorkerSelector;
use parking_lot::Mutex;
use thiserror::Error;
use tokio::{
    sync::{mpsc, oneshot, Semaphore},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    store::{ReadyTask, WorkerStore, WorkerStoreEvent},
    RawHandler, TaskContext,
};

/// Options for a [`Worker`].
#[derive(Debug)]
pub struct WorkerOptions {
    /// The amount of concurrent tasks that can be spawned.
    pub concurrent_tasks: NonZeroUsize,
    /// The timeout after which a task is forcibly cancelled
    /// after receiving a cancellation request.
    pub cancellation_timeout: Duration,
}

impl Default for WorkerOptions {
    fn default() -> Self {
        // Rather conservative by default.
        Self {
            concurrent_tasks: NonZeroUsize::new(4).unwrap(),
            cancellation_timeout: Duration::from_secs(30),
        }
    }
}

/// A worker where workers can be registered
/// and are executed whenever tasks are ready.
pub struct Worker<S> {
    store: S,
    id: Uuid,
    handlers: HashMap<WorkerSelector, Arc<dyn RawHandler + Send + Sync>>,
    semaphore: Arc<Semaphore>,
    running_tasks: Arc<Mutex<HashMap<Uuid, RunningTask>>>,
    options: WorkerOptions,
    shutdown_send: mpsc::Sender<oneshot::Sender<()>>,
    shutdown_recv: mpsc::Receiver<oneshot::Sender<()>>,
}

impl<S: std::fmt::Debug> std::fmt::Debug for Worker<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("store", &self.store)
            .field("handlers", &self.handlers.keys().collect::<Vec<_>>())
            .field("semaphore", &self.semaphore)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<S> Worker<S> {
    /// Register a handler for the worker.
    ///
    /// # Panics
    ///
    /// Panics if a handler was already registered with a matching [`WorkerSelector`].
    pub fn register_handler(&mut self, worker: Arc<dyn RawHandler + Send + Sync>) -> &mut Self {
        let selector = worker.selector();

        assert!(
            !self.handlers.contains_key(worker.selector()),
            "a worker is already registered with the given selector: {selector:?}"
        );

        self.handlers.insert(worker.selector().clone(), worker);
        self
    }
}

impl<S> Worker<S>
where
    S: WorkerStore + 'static,
{
    /// Create a new worker with the default options.
    pub fn new(store: S) -> Self {
        Self::new_with_options(store, WorkerOptions::default())
    }

    /// Create a new worker.
    pub fn new_with_options(store: S, options: WorkerOptions) -> Self {
        let (send, recv) = mpsc::channel(1);
        Self {
            store,
            id: Uuid::new_v4(),
            handlers: HashMap::new(),
            semaphore: Arc::new(Semaphore::new(options.concurrent_tasks.get())),
            running_tasks: Arc::default(),
            options,
            shutdown_send: send,
            shutdown_recv: recv,
        }
    }

    /// Get a handle to this worker.
    pub fn handle(&self) -> WorkerHandle {
        WorkerHandle {
            chan: self.shutdown_send.clone(),
        }
    }

    /// Run the worker indefinitely.
    ///
    /// # Errors
    ///
    /// The function returns on any store error.
    ///
    /// # Panics
    ///
    /// Only panics due to bugs.
    pub async fn run(mut self) -> Result<(), Error> {
        macro_rules! wait_shutdown_all {
            ($confirm:expr) => {
                let running_tasks = mem::take(&mut *self.running_tasks.lock());

                let mut tasks: FuturesUnordered<_> = running_tasks
                    .into_iter()
                    .map(|(task_id, task)| {
                        let task = task;
                        let store = self.store.clone();
                        async move {
                            tracing::warn!(%task_id, "cancelling task due to shutdown");
                            if let Err(error) =
                                store.task_cancelled(task_id).await.map_err(store_error)
                            {
                                tracing::error!(?error, "failed to cancel task");
                            }

                            task.context.cancellation.cancel();
                            let _ = task.handle.await;
                        }
                    })
                    .collect();

                while tasks.next().await.is_some() {}

                let _ = ($confirm).send(());
                return Ok(());
            };
        }

        let selectors = self.handlers.keys().cloned().collect::<Vec<_>>();

        let (rt_errors_send, mut rt_errors_recv) = mpsc::channel::<Error>(1);

        if let Ok(shutdown_confirm) = self.shutdown_recv.try_recv() {
            let _ = shutdown_confirm.send(());
            return Ok(());
        }

        let mut events = pin!(self.store.events(&selectors).await.map_err(store_error)?);

        if let Ok(shutdown_confirm) = self.shutdown_recv.try_recv() {
            let _ = shutdown_confirm.send(());
            return Ok(());
        }

        self.spawn_tasks(
            self.store
                .ready_tasks(&selectors)
                .await
                .map_err(store_error)?
                .into_iter(),
            rt_errors_send.clone(),
        )
        .await?;

        if let Ok(shutdown_confirm) = self.shutdown_recv.try_recv() {
            wait_shutdown_all!(shutdown_confirm);
        }

        loop {
            tokio::select! {
                error = rt_errors_recv.recv() => {
                    return Err(error.unwrap());
                }
                Some(shutdown_confirm) = self.shutdown_recv.recv() => {
                    wait_shutdown_all!(shutdown_confirm);
                }
                event = events.try_next() => {
                    let event = event.map_err(store_error)?.ok_or(Error::UnexpectedEventStreamEnd)?;
                    match event {
                        WorkerStoreEvent::TaskReady(task) => {
                            self.spawn_tasks(once(task), rt_errors_send.clone()).await?;
                        },
                        WorkerStoreEvent::TaskCancelled(task_id) => {
                            if let Some(task) = self.running_tasks.lock().remove(&task_id) {
                                task.context.cancellation.cancel();
                            }
                        }
                    }
                }
            }
        }
    }

    #[tracing::instrument(skip_all)]
    async fn spawn_tasks(
        &mut self,
        tasks: impl Iterator<Item = ReadyTask>,
        rt_errors: mpsc::Sender<Error>,
    ) -> Result<(), Error> {
        for task in tasks {
            let worker = self
                .handlers
                .get(&task.definition.worker_selector)
                .ok_or(Error::HandlerNotFound)?
                .clone();

            let permit = self.semaphore.clone().acquire_owned().await.unwrap();

            let should_run = self
                .store
                .select_task(task.id, self.id)
                .await
                .map_err(store_error)?;

            if !should_run {
                tracing::debug!(task_id = %task.id, "dropping task");
                continue;
            }

            let context = TaskContext {
                task_id: task.id,
                cancellation: CancellationToken::new(),
            };

            let cancellation_timeout = self.options.cancellation_timeout;
            let store = self.store.clone();
            let running_tasks = self.running_tasks.clone();

            let task_span = tracing::info_span!(
                "run_task",
                task_id = %task.id,
                kind = &*task.definition.worker_selector.kind,
            );

            let ctx = context.clone();
            let rt_errors = rt_errors.clone();

            let task_handle = tokio::spawn(async move {
                let _permit = permit;

                let cancellation = ctx.cancellation.clone();
                let mut worker_fut = worker.run(ctx, task.definition);

                if let Err(error) = store.task_started(task.id).await {
                    let _ = rt_errors.send(store_error(error)).await;
                    return;
                }

                tokio::select! {
                    _ = cancellation.cancelled() => {
                        tokio::select! {
                            _ = tokio::time::sleep(cancellation_timeout) => {}
                            res = &mut worker_fut => {
                                match res {
                                    Ok(output) => {
                                        if let Err(error) = store.task_succeeded(task.id, output, worker.output_format()).await {
                                            let _ = rt_errors.send(store_error(error)).await;
                                        }
                                    },
                                    Err(error) => {
                                        if let Err(error) = store.task_failed(task.id, format!("{error:?}")).await {
                                            let _ = rt_errors.send(store_error(error)).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    res = &mut worker_fut => {
                        match res {
                            Ok(output) => {
                                if let Err(error) = store.task_succeeded(task.id, output, worker.output_format()).await {
                                    let _ = rt_errors.send(store_error(error)).await;
                                }
                            },
                            Err(error) => {
                                if let Err(error) = store.task_failed(task.id, format!("{error:?}")).await {
                                    let _ = rt_errors.send(store_error(error)).await;
                                }
                            }
                        }
                    }
                }

                running_tasks.lock().remove(&task.id);
            }.instrument(task_span));

            self.running_tasks.lock().insert(
                task.id,
                RunningTask {
                    context,
                    handle: task_handle,
                },
            );
        }
        Ok(())
    }
}

/// A handle to a worker that can be used for graceful shutdowns.
#[derive(Debug, Clone)]
#[must_use]
pub struct WorkerHandle {
    chan: mpsc::Sender<oneshot::Sender<()>>,
}

impl WorkerHandle {
    /// Shutdown the worker by cancelling all tasks and waiting for them
    /// to finish.
    ///
    /// If the worker does not exist anymore, this is effectively a no-op.
    /// If the worker is not yet started, this will wait for the worker to start
    /// and will shut it down immediately.
    pub async fn shutdown(&self) {
        let (send, recv) = oneshot::channel();
        let _ = self.chan.send(send).await;
        let _ = recv.await;
    }
}

/// A worker error.
#[derive(Debug, Error)]
pub enum Error {
    /// A specific handler was not found, but the
    /// still received the task. This
    /// is either a bug in the worker selector
    /// or the store.
    #[error("received task but no matching handler was found")]
    HandlerNotFound,
    /// The store event stream ended unexpectedly.
    #[error("unexpected end of event stream")]
    UnexpectedEventStreamEnd,
    /// A store error.
    #[error("store error: {0:?}")]
    Store(Box<dyn std::error::Error + Send + Sync>),
}

struct RunningTask {
    context: TaskContext,
    handle: JoinHandle<()>,
}

fn store_error<E: std::error::Error + Send + Sync + 'static>(error: E) -> Error {
    Error::Store(Box::new(error))
}
