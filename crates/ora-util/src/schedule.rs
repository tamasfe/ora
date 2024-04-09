//! Schedule utilities.

use chrono::TimeZone;
use ora_common::{
    schedule::{MissedTasksPolicy, ScheduleDefinition, SchedulePolicy},
    UnixNanos,
};

/// Return the next task time for a schedule.
#[must_use]
pub fn next_schedule_task(
    schedule: &ScheduleDefinition,
    last_task: Option<UnixNanos>,
    time: UnixNanos,
) -> Option<UnixNanos> {
    match last_task {
        Some(last) => match schedule.missed_tasks {
            MissedTasksPolicy::Burst => next_from(schedule, last),
            MissedTasksPolicy::Skip => {
                let mut next = next_from(schedule, last)?;

                while next < time {
                    next = next_from(schedule, next)?;
                }

                Some(next)
            }
        },
        None => {
            if schedule.immediate {
                Some(time)
            } else {
                next_from(schedule, time)
            }
        }
    }
}

/// Return the next execution target time from the given point
/// in time.
#[allow(clippy::cast_possible_wrap)]
fn next_from(schedule: &ScheduleDefinition, time: UnixNanos) -> Option<UnixNanos> {
    match &schedule.policy {
        SchedulePolicy::Repeat { interval_ns, .. } => Some(UnixNanos(time.0 + interval_ns)),
        SchedulePolicy::Cron { expr, .. } => {
            let c: cron::Schedule = match expr.parse() {
                Ok(c) => c,
                Err(error) => {
                    tracing::error!(%error, "invalid cron expression");
                    return None;
                }
            };

            c.after(&chrono::Utc.timestamp_nanos(time.0 as _))
                .next()
                .and_then(|t| {
                    Some(UnixNanos(
                        t.timestamp_nanos_opt()
                            .unwrap_or_default()
                            .try_into()
                            .ok()?,
                    ))
                })
        }
    }
}
