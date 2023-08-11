//! Worker pool implementation.

use std::{
    collections::HashMap, iter::once, num::NonZeroUsize, pin::pin, sync::Arc, time::Duration,
};

use futures::TryStreamExt;
use ora_common::task::WorkerSelector;
use parking_lot::Mutex;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    store::{ReadyTask, WorkerPoolStore, WorkerPoolStoreEvent},
    RawWorker, TaskContext,
};

/// Options for a [`WorkerPool`].
#[derive(Debug)]
pub struct WorkerPoolOptions {
    /// The amount of concurrent tasks that can be spawned.
    concurrent_tasks: NonZeroUsize,
    /// The timeout after which a task is forcibly cancelled
    /// after receiving a cancellation request.
    cancellation_timeout: Duration,
}

impl Default for WorkerPoolOptions {
    fn default() -> Self {
        // Rather conservative by default.
        Self {
            concurrent_tasks: NonZeroUsize::new(4).unwrap(),
            cancellation_timeout: Duration::from_secs(30),
        }
    }
}

/// A worker pool where workers can be registered
/// and are executed whenever tasks are ready.
pub struct WorkerPool<S> {
    store: S,
    workers: HashMap<WorkerSelector, Arc<dyn RawWorker + Send + Sync>>,
    semaphore: Arc<Semaphore>,
    running_tasks: Arc<Mutex<HashMap<Uuid, RunningTask>>>,
    options: WorkerPoolOptions,
}

impl<S: std::fmt::Debug> std::fmt::Debug for WorkerPool<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerPool")
            .field("store", &self.store)
            .field("workers", &self.workers.keys().collect::<Vec<_>>())
            .field("semaphore", &self.semaphore)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<S> WorkerPool<S> {
    /// Register a worker in the pool.
    ///
    /// # Panics
    ///
    /// Panics if a worker was already registered with a matching [`WorkerSelector`].
    pub fn register_worker(&mut self, worker: Arc<dyn RawWorker + Send + Sync>) -> &mut Self {
        let selector = worker.selector();

        assert!(
            !self.workers.contains_key(worker.selector()),
            "a worker is already registered with the given selector: {selector:?}"
        );

        self.workers.insert(worker.selector().clone(), worker);
        self
    }
}

impl<S> WorkerPool<S>
where
    S: WorkerPoolStore + 'static,
{
    /// Create a new worker pool with the default options.
    pub fn new(store: S) -> Self {
        Self::new_with_options(store, WorkerPoolOptions::default())
    }

    /// Create a new worker pool.
    pub fn new_with_options(store: S, options: WorkerPoolOptions) -> Self {
        Self {
            store,
            workers: HashMap::new(),
            semaphore: Arc::new(Semaphore::new(options.concurrent_tasks.get())),
            running_tasks: Arc::default(),
            options,
        }
    }

    /// Run the worker pool indefinitely.
    ///
    /// # Errors
    ///
    /// The function returns on any store error.
    ///
    /// # Panics
    ///
    /// Only panics due to bugs.
    pub async fn run(mut self) -> Result<(), Error> {
        let selectors = self.workers.keys().cloned().collect::<Vec<_>>();

        let (rt_errors_send, mut rt_errors_recv) = tokio::sync::mpsc::channel::<Error>(1);

        let mut events = pin!(self.store.events(&selectors).await.map_err(store_error)?);

        self.spawn_tasks(
            self.store
                .ready_tasks(&selectors)
                .await
                .map_err(store_error)?
                .into_iter(),
            rt_errors_send.clone(),
        )
        .await?;

        loop {
            tokio::select! {
                error = rt_errors_recv.recv() => {
                    return Err(error.unwrap());
                }
                event = events.try_next() => {
                    let event = event.map_err(store_error)?.ok_or(Error::UnexpectedEventStreamEnd)?;
                    match event {
                        WorkerPoolStoreEvent::TaskReady(task) => {
                            self.spawn_tasks(once(task), rt_errors_send.clone()).await?;
                        },
                        WorkerPoolStoreEvent::TaskCancelled(task_id) => {
                            if let Some(task) = self.running_tasks.lock().remove(&task_id) {
                                task.context.cancellation.cancel();
                            }
                        }
                    }
                }
            }
        }
    }

    #[inline]
    async fn spawn_tasks(
        &mut self,
        tasks: impl Iterator<Item = ReadyTask>,
        rt_errors: tokio::sync::mpsc::Sender<Error>,
    ) -> Result<(), Error> {
        for task in tasks {
            let worker = self
                .workers
                .get(&task.definition.worker_selector)
                .ok_or(Error::WorkerNotFound)?
                .clone();

            let permit = self.semaphore.clone().acquire_owned().await.unwrap();

            let context = TaskContext {
                task_id: task.id,
                cancellation: CancellationToken::new(),
            };

            self.running_tasks.lock().insert(
                task.id,
                RunningTask {
                    context: context.clone(),
                },
            );

            let cancellation_timeout = self.options.cancellation_timeout;
            let store = self.store.clone();
            let running_tasks = self.running_tasks.clone();

            let rt_errors = rt_errors.clone();
            tokio::spawn(async move {
                let _permit = permit;

                let cancellation = context.cancellation.clone();
                let mut worker_fut = worker.run(context, task.definition);

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
            });
        }
        Ok(())
    }
}

/// A worker pool error.
#[derive(Debug, Error)]
pub enum Error {
    /// A specific worker was not found, but the
    /// pool still received the task. This
    /// is either a bug in the worker selector
    /// or the store.
    #[error("received task but no matching worker was found")]
    WorkerNotFound,
    /// The store event stream ended unexpectedly.
    #[error("unexpected end of event stream")]
    UnexpectedEventStreamEnd,
    /// A store error.
    #[error("store error: {0:?}")]
    Store(Box<dyn std::error::Error + Send + Sync>),
}

struct RunningTask {
    context: TaskContext,
}

fn store_error<E: std::error::Error + Send + Sync + 'static>(error: E) -> Error {
    Error::Store(Box::new(error))
}
