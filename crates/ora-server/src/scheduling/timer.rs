use std::thread::{self, JoinHandle};
use std::{io, time::Duration};

use ora_timer::{resolution::MillisecondResolution, Delayed, TimerOptions};
use uuid::Uuid;
use wgroup::WaitGuard;

use crate::{
    events::{EventBus, ExecutionEvent},
    time::UnixNanos,
};

use ora_storage::PendingExecution;

pub fn spawn_timer(
    mut ready_executions: rtrb::Producer<Uuid>,
    mut pending_executions: rtrb::Consumer<PendingExecution>,
    wg: WaitGuard,
    timer_options: TimerOptions,
    event_bus: EventBus,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new().name("timer".into()).spawn(move || {
        ora_timer::run_hierarchical_timer::<Uuid, MillisecondResolution>(
            timer_options,
            |new_timed, ready_timed| {
                if wg.is_waiting() {
                    return ora_timer::TimerLoopAction::Stop;
                }

                write_all_ready(ready_timed, &mut ready_executions, &event_bus);

                if pending_executions.is_empty() {
                    return if wg.is_waiting() {
                        ora_timer::TimerLoopAction::Stop
                    } else {
                        ora_timer::TimerLoopAction::Continue
                    };
                }

                let pending_buf = pending_executions
                    .read_chunk(pending_executions.slots())
                    .unwrap();

                // We save a roundtrip to the timer for jobs
                // that are already ready to execute.
                let mut ready_timed = Vec::new();

                let now = UnixNanos::now();

                new_timed.extend(pending_buf.into_iter().filter_map(|pending| {
                    let now_ts = now.0;
                    let target_ts = UnixNanos::from(pending.target_execution_time).0;

                    let delay = Duration::from_nanos(target_ts.saturating_sub(now_ts));

                    if delay == Duration::ZERO {
                        ready_timed.push(pending.id);
                        return None;
                    }

                    Some(Delayed::new(pending.id, delay))
                }));

                write_all_ready(&mut ready_timed, &mut ready_executions, &event_bus);

                if wg.is_waiting() {
                    ora_timer::TimerLoopAction::Stop
                } else {
                    ora_timer::TimerLoopAction::Continue
                }
            },
        );
    })
}

fn write_all_ready(
    ready_timed: &mut Vec<Uuid>,
    ready_executions: &mut rtrb::Producer<Uuid>,
    event_bus: &EventBus,
) {
    if ready_timed.is_empty() {
        return;
    }

    if ready_executions.is_full() {
        tracing::warn!(
            "ready executions are not being consumed fast enough, consider using a larger buffer"
        );
    }

    let mut ready_timed = ready_timed.drain(..);

    loop {
        let Ok(ready_buf) = ready_executions.write_chunk_uninit(ready_executions.slots()) else {
            // The buffer is full, we need to wait for the consumer to catch up.
            continue;
        };

        ready_buf.fill_from_iter(&mut ready_timed);

        if ready_timed.len() == 0 {
            break;
        }
    }

    event_bus.emit_execution_event(ExecutionEvent::TimedExecutionsReady);
}
