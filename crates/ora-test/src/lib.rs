//! Test utilities for Ora.
#![warn(clippy::pedantic, missing_docs)]
#![allow(clippy::ignored_unit_patterns)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use ora_api::{client::TaskHandle, Handler, Task};
use ora_client::{RawTaskResult, ScheduleOperations, TaskOperations};
use ora_common::{
    task::{TaskDefinition, TaskStatus, WorkerSelector},
    UnixNanos,
};
use ora_worker::RawHandler;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// A worker that can be used to test worker implementations.
#[derive(Default)]
#[must_use]
pub struct TestWorker {
    workers: HashMap<WorkerSelector, Arc<dyn RawHandler + Send + Sync>>,
}

impl std::fmt::Debug for TestWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestWorker")
            .field("workers", &self.workers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl TestWorker {
    /// Create a new test worker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a handler to the test worker.
    ///
    /// # Panics
    ///
    /// Panics if a handler with a matching [`WorkerSelector`]
    /// already exists.
    pub fn register_handler(&mut self, worker: Arc<dyn RawHandler + Send + Sync>) -> &mut Self {
        let selector = worker.selector();

        assert!(
            !self.workers.contains_key(worker.selector()),
            "a worker is already registered with the given selector: {selector:?}"
        );

        self.workers.insert(worker.selector().clone(), worker);
        self
    }

    /// Spawn a task onto the worker, immediately run it with a
    /// registered and return a [`TaskHandle`] for it.
    ///
    /// It returns [`None`] if there are no suitable workers registered.
    ///
    /// # Panics
    ///
    /// Panics if not called inside a [`tokio`] runtime.
    pub fn spawn_task<T>(&mut self, task: TaskDefinition<T>) -> Option<TaskHandle<T>>
    where
        T: Send + 'static,
    {
        let worker = self.workers.get(&task.worker_selector)?.clone();

        let task_id = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        let ctx = ora_worker::_private::new_context(task_id, cancellation.clone());

        let inner = Arc::new(Mutex::new(TestTaskInner {
            task_id,
            definition: task.clone().cast(),
            cancellation,
            added: UnixNanos::now(),
            finished: None,
            result: None,
            cancelled: None,
        }));

        let task_inner = inner.clone();
        tokio::spawn(async move {
            let data_format = task.data_format;

            let res = worker.run(ctx, task.cast()).await;
            let mut inner = task_inner.lock().unwrap();
            if inner.result.is_some() {
                return;
            }

            inner.finished = Some(UnixNanos::now());

            match res {
                Ok(output) => {
                    inner.result = Some(RawTaskResult::Success {
                        output_format: data_format,
                        output,
                    });
                }
                Err(error) => {
                    inner.result = Some(RawTaskResult::Failure {
                        reason: format!("{error:?}"),
                    });
                }
            }
        });

        Some(TaskHandle::new_raw(Arc::new(TestTaskOperations { inner })))
    }
}

/// Run a worker with a given task and return its output.
///
/// This will simply provide an empty context to the worker and
/// run the handler function.
/// For more options, see [`TestWorker`].
///
/// # Errors
///
/// Returns errors returned by the worker.
pub async fn run_worker<T, W>(worker: &W, task: T) -> eyre::Result<T::Output>
where
    T: Task,
    W: Handler<T>,
{
    worker
        .run(
            ora_worker::_private::new_context(Uuid::nil(), CancellationToken::default()),
            task,
        )
        .await
}

#[derive(Debug)]
struct TestTaskOperations {
    inner: Arc<Mutex<TestTaskInner>>,
}

#[derive(Debug)]
struct TestTaskInner {
    task_id: Uuid,
    definition: TaskDefinition,
    cancellation: CancellationToken,
    added: UnixNanos,
    finished: Option<UnixNanos>,
    result: Option<RawTaskResult>,
    cancelled: Option<UnixNanos>,
}

#[async_trait]
impl TaskOperations for TestTaskOperations {
    fn id(&self) -> Uuid {
        self.inner.lock().unwrap().task_id
    }

    async fn status(&self) -> eyre::Result<TaskStatus> {
        match &self.inner.lock().unwrap().result {
            Some(res) => match res {
                RawTaskResult::Success { .. } => Ok(TaskStatus::Succeeded),
                RawTaskResult::Failure { .. } => Ok(TaskStatus::Failed),
                RawTaskResult::Cancelled => Ok(TaskStatus::Cancelled),
            },
            None => Ok(TaskStatus::Started),
        }
    }

    async fn target(&self) -> eyre::Result<UnixNanos> {
        Ok(self.inner.lock().unwrap().definition.target)
    }

    async fn definition(&self) -> eyre::Result<TaskDefinition> {
        Ok(self.inner.lock().unwrap().definition.clone())
    }

    async fn result(&self) -> eyre::Result<Option<RawTaskResult>> {
        Ok(self.inner.lock().unwrap().result.clone())
    }

    async fn wait_result(&self) -> eyre::Result<RawTaskResult> {
        loop {
            if let Some(res) = self.result().await? {
                return Ok(res);
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    async fn schedule(&self) -> eyre::Result<Option<Arc<dyn ScheduleOperations>>> {
        Ok(None)
    }

    async fn added_at(&self) -> eyre::Result<UnixNanos> {
        Ok(self.inner.lock().unwrap().added)
    }

    async fn ready_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(Some(self.inner.lock().unwrap().added))
    }

    async fn started_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(Some(self.inner.lock().unwrap().added))
    }

    async fn succeeded_at(&self) -> eyre::Result<Option<UnixNanos>> {
        let inner = self.inner.lock().unwrap();

        if let Some(res) = &inner.result {
            if res.is_success() {
                return Ok(inner.finished);
            }
        }

        Ok(None)
    }

    async fn failed_at(&self) -> eyre::Result<Option<UnixNanos>> {
        let inner = self.inner.lock().unwrap();

        if let Some(res) = &inner.result {
            if res.is_failure() {
                return Ok(inner.finished);
            }
        }

        Ok(None)
    }

    async fn cancelled_at(&self) -> eyre::Result<Option<UnixNanos>> {
        Ok(self.inner.lock().unwrap().cancelled)
    }

    async fn cancel(&self) -> eyre::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.result.is_none() {
            inner.cancelled = Some(UnixNanos::now());
            inner.cancellation.cancel();
            inner.result = Some(RawTaskResult::Cancelled);
        }

        Ok(())
    }

    async fn worker_id(&self) -> eyre::Result<Option<Uuid>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use ora_api::{Handler, IntoHandler, Task};
    use ora_worker::TaskContext;
    use serde::{Deserialize, Serialize};
    use tokio::test;
    use uuid::Uuid;

    use crate::TestWorker;

    #[derive(Debug, Serialize, Deserialize)]
    struct TestTask;

    impl Task for TestTask {
        type Output = Uuid;
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct CancelOnlyTask;

    impl Task for CancelOnlyTask {
        type Output = ();
    }

    struct TestHandler;

    #[async_trait]
    impl Handler<TestTask> for TestHandler {
        async fn run(
            &self,
            ctx: TaskContext,
            _task: TestTask,
        ) -> eyre::Result<<TestTask as Task>::Output> {
            Ok(ctx.task_id())
        }
    }

    #[async_trait]
    impl Handler<CancelOnlyTask> for TestHandler {
        async fn run(
            &self,
            ctx: TaskContext,
            _task: CancelOnlyTask,
        ) -> eyre::Result<<CancelOnlyTask as Task>::Output> {
            ctx.cancelled().await;
            Ok(())
        }
    }

    #[test]
    async fn test_worker_smoke() {
        let mut worker = TestWorker::new();
        assert!(worker.spawn_task(TestTask.task()).is_none());
        worker.register_handler(TestHandler.handler::<TestTask>());

        let task = worker.spawn_task(TestTask.task()).unwrap();
        let output_task_id = task.clone().await.unwrap();
        assert_eq!(output_task_id, task.id());

        worker.register_handler(TestHandler.handler::<CancelOnlyTask>());

        let task = worker.spawn_task(CancelOnlyTask.task()).unwrap();
        tokio::select! {
            _ = task.clone() => {
                unreachable!()
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {},
        }

        task.cancel().await.unwrap();
        assert!(task.cancelled_at().await.unwrap().is_some());
        assert!(task.await.is_err());
    }

    #[test]
    #[should_panic = "handler registered multipl times."]
    async fn test_duplicate_handlers() {
        let mut worker = TestWorker::new();
        worker.register_handler(TestHandler.handler::<TestTask>());
        worker.register_handler(TestHandler.handler::<TestTask>());
    }
}
