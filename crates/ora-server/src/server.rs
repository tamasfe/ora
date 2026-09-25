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
        executions::{
            execution_starts_loop, execution_timeouts_loop, executor_events_loop,
            ready_executions_loop,
        },
        schedules::schedule_new_jobs_loop,
    },
};

mod delete_history;
mod executions;
mod schedules;

pub(crate) use schedules::validate_schedule;

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

    let (executor_pool, executor_events, accepted_executions) = {
        let (events_send, events_recv) = flume::unbounded();
        let (accepted_send, accepted_recv) = flume::unbounded();
        let pool = ExecutorPool::new(
            events_send,
            accepted_send,
            handle.clone(),
            options.shutdown_grace_period,
        );
        (pool, events_recv, accepted_recv)
    };

    spawn(ready_executions_loop(
        backend.clone(),
        executor_pool.clone(),
        handle.add_with("ready_executions"),
    ));
    spawn(execution_starts_loop(
        backend.clone(),
        executor_pool.clone(),
        accepted_executions,
        handle.add_with("execution_starts"),
    ));
    spawn(executor_events_loop(
        backend.clone(),
        executor_pool.clone(),
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
