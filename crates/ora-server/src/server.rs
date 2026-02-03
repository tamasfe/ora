//! The server implementation for Ora.

use either::Either;
use ora_backend::Backend;
use tokio::spawn;
use wgroup::{WaitGroup, WaitGroupHandle};

use crate::{
    ServerHandle, ServerOptions,
    executor_pool::ExecutorPool,
    server::{
        delete_history::delete_history_loop,
        executions::{execution_timeouts_loop, executor_events_loop, ready_executions_loop},
        schedules::schedule_new_jobs_loop,
    },
};

mod delete_history;
mod executions;
mod schedules;

pub(crate) fn spawn_server<B>(
    backend: std::sync::Arc<B>,
    options: ServerOptions,
    wg: Either<WaitGroup, WaitGroupHandle>,
) -> ServerHandle<B>
where
    B: Backend + 'static,
{
    let handle = match &wg {
        Either::Left(wg) => wg.handle(),
        Either::Right(handle) => handle.clone(),
    };

    let (executor_pool, executor_events) = {
        let (send, recv) = flume::unbounded();
        let pool = ExecutorPool::new(send, handle.clone(), options.shutdown_grace_period);
        (pool, recv)
    };

    spawn(ready_executions_loop(
        backend.clone(),
        executor_pool.clone(),
        handle.add_with("ready_executions"),
    ));
    spawn(executor_events_loop(
        backend.clone(),
        executor_events,
        handle.add_with("executor_events"),
    ));
    spawn(execution_timeouts_loop(
        backend.clone(),
        executor_pool.clone(),
        handle.add_with("execution_timeouts"),
    ));
    spawn(schedule_new_jobs_loop(
        backend.clone(),
        handle.add_with("scheduling_new_jobs"),
    ));

    if !options.delete_history_after.is_zero() {
        spawn(delete_history_loop(
            backend.clone(),
            options.delete_history_after,
            handle.add_with("delete_history"),
        ));
    }

    ServerHandle {
        backend,
        executor_pool,
        wg,
    }
}
