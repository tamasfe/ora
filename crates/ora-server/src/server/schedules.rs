use futures::StreamExt;
use jiff::Timestamp;
use std::{
    pin::pin,
    sync::Arc,
    time::{Duration, SystemTime},
};
use wgroup::WaitGuard;

use ora_backend::{
    Backend,
    jobs::{JobDefinition, NewJob},
    schedules::{
        MissedTimePolicy, PendingSchedule, ScheduleDefinition, ScheduleFilters, ScheduleId,
        SchedulingPolicy,
    },
};
use uuid::Uuid;

#[tracing::instrument(skip_all)]
pub(super) async fn schedule_new_jobs_loop(backend: Arc<impl Backend>, wg: WaitGuard) {
    loop {
        let mut stream = pin!(backend.pending_schedules());

        // Schedules that could not be handled stay pending,
        // waiting for pending schedules would return immediately.
        let mut unhandled_schedules = false;

        while let Some(pending_schedules) = stream.next().await {
            let pending_schedules = match pending_schedules {
                Ok(s) => s,
                Err(error) => {
                    tracing::error!(%error, "failed to retrieve pending schedules");
                    unhandled_schedules = true;
                    break;
                }
            };

            let now = SystemTime::now();

            let mut new_jobs = Vec::with_capacity(pending_schedules.len());
            let mut stop_scheduling_ids = Vec::new();

            for schedule in pending_schedules {
                let next_time = match next_execution_time(now, &schedule) {
                    Ok(Some(s)) => s,
                    Ok(None) => {
                        tracing::debug!(
                            schedule_id = %schedule.schedule_id,
                            "schedule has no more execution times, skipping job creation",
                        );
                        stop_scheduling_ids.push(schedule.schedule_id);
                        continue;
                    }
                    Err(error) => {
                        tracing::error!(
                            %error,
                            schedule_id = %schedule.schedule_id,
                            "failed to determine execution time for schedule",
                        );
                        unhandled_schedules = true;
                        continue;
                    }
                };

                if let Some(end_time) = schedule.time_range.end
                    && next_time >= end_time
                {
                    tracing::debug!(
                        schedule_id = %schedule.schedule_id,
                        "schedule has passed its end time, skipping job creation",
                    );
                    stop_scheduling_ids.push(schedule.schedule_id);
                    continue;
                }

                new_jobs.push(NewJob {
                    job: JobDefinition {
                        target_execution_time: next_time,
                        ..schedule.job_template
                    },
                    schedule_id: Some(schedule.schedule_id),
                });
            }

            if !new_jobs.is_empty() {
                match backend.add_jobs(&new_jobs, None).await {
                    Ok(jobs) => {
                        tracing::debug!(
                            job_count = jobs.added_job_ids().len(),
                            "spawned new jobs for schedules"
                        );
                    }
                    Err(error) => {
                        tracing::error!(%error, "failed to spawn jobs for schedules");
                        unhandled_schedules = true;
                    }
                }
            }

            if !stop_scheduling_ids.is_empty() {
                match backend
                    .stop_schedules(ScheduleFilters {
                        schedule_ids: Some(stop_scheduling_ids),
                        ..Default::default()
                    })
                    .await
                {
                    Ok(stopped) => {
                        tracing::debug!(
                            schedule_count = stopped.len(),
                            "stopped scheduling for ended schedules",
                        );
                    }
                    Err(error) => {
                        tracing::error!(%error, "failed to stop scheduling for ended schedules");
                        unhandled_schedules = true;
                    }
                }
            }
        }

        let check_delay = tokio::time::sleep(std::time::Duration::from_secs(5));

        tokio::select! {
            _ = wg.waiting() => {
                tracing::debug!("shutting down");
                return;
            }
            _ = backend.wait_for_pending_schedules(), if !unhandled_schedules => {}
            _ = check_delay => {
                tracing::trace!("periodic check for pending schedules");
            }
        }
    }
}

/// An error determining the next execution time of a schedule.
#[derive(Debug)]
pub(crate) enum ScheduleTimeError {
    /// The interval of the schedule is zero.
    ZeroInterval,
    /// A time is outside of the supported range.
    OutOfRange,
    /// The cron expression is invalid.
    Cron(cronexpr::Error),
    /// The schedule has no execution times.
    NoExecutionTimes,
}

impl std::fmt::Display for ScheduleTimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleTimeError::ZeroInterval => f.write_str("the interval must not be zero"),
            ScheduleTimeError::OutOfRange => f.write_str("time is out of the supported range"),
            ScheduleTimeError::Cron(error) => write!(f, "invalid cron expression: {error}"),
            ScheduleTimeError::NoExecutionTimes => {
                f.write_str("the schedule has no execution times")
            }
        }
    }
}

impl core::error::Error for ScheduleTimeError {}

impl From<cronexpr::Error> for ScheduleTimeError {
    fn from(error: cronexpr::Error) -> Self {
        Self::Cron(error)
    }
}

/// Validate a new schedule by determining its first execution time.
pub(crate) fn validate_schedule(
    now: SystemTime,
    schedule: &ScheduleDefinition,
) -> Result<(), ScheduleTimeError> {
    // Not creating the first job immediately
    // also validates the regular execution times.
    let scheduling = match &schedule.scheduling {
        SchedulingPolicy::FixedInterval {
            interval, missed, ..
        } => SchedulingPolicy::FixedInterval {
            interval: *interval,
            immediate: false,
            missed: *missed,
        },
        SchedulingPolicy::Cron {
            expression, missed, ..
        } => SchedulingPolicy::Cron {
            expression: expression.clone(),
            immediate: false,
            missed: *missed,
        },
    };

    next_execution_time(
        now,
        &PendingSchedule {
            schedule_id: ScheduleId(Uuid::nil()),
            last_target_execution_time: None,
            scheduling,
            time_range: schedule.time_range.clone(),
            job_template: schedule.job_template.clone(),
        },
    )?
    .map(|_| ())
    .ok_or(ScheduleTimeError::NoExecutionTimes)
}

fn duration_from_nanos(nanos: u128) -> Option<Duration> {
    const NANOS_PER_SEC: u128 = 1_000_000_000;

    Some(Duration::new(
        u64::try_from(nanos / NANOS_PER_SEC).ok()?,
        (nanos % NANOS_PER_SEC) as u32,
    ))
}

fn to_timestamp(time: SystemTime) -> Result<Timestamp, ScheduleTimeError> {
    Timestamp::try_from(time).map_err(|_| ScheduleTimeError::OutOfRange)
}

/// Add a duration to a time, the result must be in the supported range.
fn add_time(time: SystemTime, duration: Duration) -> Result<SystemTime, ScheduleTimeError> {
    let time = time
        .checked_add(duration)
        .ok_or(ScheduleTimeError::OutOfRange)?;

    // Times that `SystemTime` supports might not be supported by
    // the backend, the range of timestamps is the same as for cron schedules.
    to_timestamp(time)?;

    Ok(time)
}

/// Find the next execution time after `time` of a cron expression,
/// `None` is returned if there is none.
fn find_next_cron(cron: &cronexpr::Crontab, time: Timestamp) -> Option<SystemTime> {
    // This fails if there is no matching time in the next
    // few years (e.g. February 30th) or the time range ends.
    cron.find_next(time)
        .ok()
        .map(|next| next.timestamp().into())
}

/// Determine the next execution time of a pending schedule.
///
/// Returns `None` if the schedule has no more execution times.
fn next_execution_time(
    mut now: SystemTime,
    schedule: &PendingSchedule,
) -> Result<Option<SystemTime>, ScheduleTimeError> {
    if let Some(start_time) = schedule.time_range.start
        && now < start_time
    {
        // we can still schedule a job ahead of time.
        now = start_time;
    }

    match &schedule.scheduling {
        SchedulingPolicy::FixedInterval {
            interval,
            immediate,
            missed,
        } => {
            if interval.is_zero() {
                return Err(ScheduleTimeError::ZeroInterval);
            }

            if let Some(last_time) = schedule.last_target_execution_time {
                let next_time = add_time(last_time, *interval)?;

                match missed {
                    MissedTimePolicy::Skip => {
                        let Ok(behind) = now.duration_since(next_time) else {
                            return Ok(Some(next_time));
                        };

                        // The smallest multiple of the interval that is not before now.
                        let missed_intervals = behind.as_nanos().div_ceil(interval.as_nanos());

                        let skipped = missed_intervals
                            .checked_mul(interval.as_nanos())
                            .and_then(duration_from_nanos)
                            .ok_or(ScheduleTimeError::OutOfRange)?;

                        Ok(Some(add_time(next_time, skipped)?))
                    }
                    MissedTimePolicy::Create => Ok(Some(next_time)),
                }
            } else if *immediate {
                to_timestamp(now)?;
                Ok(Some(now))
            } else {
                Ok(Some(add_time(now, *interval)?))
            }
        }
        SchedulingPolicy::Cron {
            expression,
            immediate,
            missed,
        } => {
            let mut cron_parse_options = cronexpr::ParseOptions::default();
            cron_parse_options.fallback_timezone_option = cronexpr::FallbackTimezoneOption::System;

            let cron = cronexpr::parse_crontab_with(expression, cron_parse_options)?;

            let now_ts = to_timestamp(now)?;

            if let Some(last_time) = schedule.last_target_execution_time {
                match missed {
                    // The next time must be after the last one even if
                    // the clock is behind the one that determined when
                    // the last job was executed, otherwise the same
                    // time would be scheduled again.
                    MissedTimePolicy::Skip => {
                        let after =
                            to_timestamp(last_time).map_or(now_ts, |last_ts| now_ts.max(last_ts));
                        Ok(find_next_cron(&cron, after))
                    }
                    MissedTimePolicy::Create => Ok(find_next_cron(&cron, to_timestamp(last_time)?)),
                }
            } else if *immediate {
                Ok(Some(now))
            } else {
                Ok(find_next_cron(&cron, now_ts))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use ora_backend::{
        common::TimeRange,
        jobs::{JobTypeId, RetryPolicy, TimeoutBaseTime, TimeoutPolicy},
    };

    use super::*;

    fn schedule(scheduling: SchedulingPolicy, last: Option<SystemTime>) -> PendingSchedule {
        PendingSchedule {
            schedule_id: ScheduleId(Uuid::nil()),
            last_target_execution_time: last,
            scheduling,
            time_range: TimeRange::default(),
            job_template: JobDefinition {
                job_type_id: JobTypeId::new("Test").unwrap(),
                target_execution_time: UNIX_EPOCH,
                input_payload_json: "{}".to_string(),
                labels: vec![],
                timeout_policy: TimeoutPolicy {
                    timeout: Duration::ZERO,
                    base_time: TimeoutBaseTime::StartTime,
                },
                retry_policy: RetryPolicy::default(),
                priority: 0,
            },
        }
    }

    fn interval(interval: Duration, missed: MissedTimePolicy) -> SchedulingPolicy {
        SchedulingPolicy::FixedInterval {
            interval,
            immediate: false,
            missed,
        }
    }

    fn cron(expression: &str) -> SchedulingPolicy {
        SchedulingPolicy::Cron {
            expression: expression.to_string(),
            immediate: false,
            missed: MissedTimePolicy::Skip,
        }
    }

    #[test]
    fn zero_interval_is_an_error() {
        let now = UNIX_EPOCH + Duration::from_secs(1000);

        for missed in [MissedTimePolicy::Skip, MissedTimePolicy::Create] {
            let schedule = schedule(interval(Duration::ZERO, missed), Some(UNIX_EPOCH));
            assert!(matches!(
                next_execution_time(now, &schedule),
                Err(ScheduleTimeError::ZeroInterval)
            ));
        }
    }

    #[test]
    fn skip_missed_intervals() {
        let last = UNIX_EPOCH + Duration::from_secs(100);
        let schedule = schedule(
            interval(Duration::from_secs(10), MissedTimePolicy::Skip),
            Some(last),
        );

        // Not behind.
        let now = UNIX_EPOCH + Duration::from_secs(105);
        assert_eq!(
            next_execution_time(now, &schedule).unwrap(),
            Some(UNIX_EPOCH + Duration::from_secs(110))
        );

        // Exactly on an interval.
        let now = UNIX_EPOCH + Duration::from_secs(150);
        assert_eq!(
            next_execution_time(now, &schedule).unwrap(),
            Some(UNIX_EPOCH + Duration::from_secs(150))
        );

        // Between intervals.
        let now = UNIX_EPOCH + Duration::from_secs(151);
        assert_eq!(
            next_execution_time(now, &schedule).unwrap(),
            Some(UNIX_EPOCH + Duration::from_secs(160))
        );

        // Far behind with a tiny interval.
        let schedule = self::schedule(
            interval(Duration::from_nanos(1), MissedTimePolicy::Skip),
            Some(UNIX_EPOCH),
        );
        let now = UNIX_EPOCH + Duration::from_hours(100 * 365 * 24);
        assert_eq!(next_execution_time(now, &schedule).unwrap(), Some(now));
    }

    #[test]
    fn huge_interval_does_not_panic() {
        let now = UNIX_EPOCH + Duration::from_secs(1000);

        for last in [None, Some(now)] {
            for missed in [MissedTimePolicy::Skip, MissedTimePolicy::Create] {
                let schedule = schedule(interval(Duration::MAX, missed), last);
                assert!(matches!(
                    next_execution_time(now, &schedule),
                    Err(ScheduleTimeError::OutOfRange)
                ));
            }
        }
    }

    #[test]
    fn cron_out_of_range_does_not_panic() {
        // Beyond the year 9999.
        let far_future = UNIX_EPOCH + Duration::from_hours(30_000 * 365 * 24);

        let mut schedule = schedule(cron("0 0 * * * UTC"), None);
        schedule.time_range.start = Some(far_future);

        assert!(matches!(
            next_execution_time(UNIX_EPOCH, &schedule),
            Err(ScheduleTimeError::OutOfRange)
        ));

        let schedule = self::schedule(cron("0 0 * * * UTC"), Some(far_future));

        assert!(matches!(
            next_execution_time(UNIX_EPOCH, &schedule),
            Ok(Some(_))
        ));
    }

    #[test]
    fn cron_without_times() {
        let now = UNIX_EPOCH + Duration::from_secs(1000);

        let schedule = schedule(cron("0 0 30 2 * UTC"), None);
        assert!(matches!(next_execution_time(now, &schedule), Ok(None)));

        let definition = ScheduleDefinition {
            scheduling: schedule.scheduling.clone(),
            job_template: schedule.job_template.clone(),
            labels: vec![],
            time_range: TimeRange::default(),
        };
        assert!(matches!(
            validate_schedule(now, &definition),
            Err(ScheduleTimeError::NoExecutionTimes)
        ));
    }

    #[test]
    fn cron_skip_is_after_last_time() {
        let last = UNIX_EPOCH + Duration::from_hours(1);
        let mut schedule = schedule(cron("0 * * * * UTC"), Some(last));

        // The clock is slightly behind the last target time.
        let now = last - Duration::from_millis(100);

        assert_eq!(
            next_execution_time(now, &schedule).unwrap(),
            Some(last + Duration::from_hours(1))
        );

        schedule.scheduling = SchedulingPolicy::Cron {
            expression: "0 * * * * UTC".to_string(),
            immediate: false,
            missed: MissedTimePolicy::Create,
        };

        assert_eq!(
            next_execution_time(now, &schedule).unwrap(),
            Some(last + Duration::from_hours(1))
        );
    }

    #[test]
    fn interval_beyond_supported_range() {
        let now = UNIX_EPOCH + Duration::from_secs(1000);
        // Representable by `SystemTime`, but beyond the year 9999.
        let interval = Duration::from_hours(20_000 * 365 * 24);

        for last in [None, Some(now)] {
            for missed in [MissedTimePolicy::Skip, MissedTimePolicy::Create] {
                let schedule = schedule(self::interval(interval, missed), last);
                assert!(matches!(
                    next_execution_time(now, &schedule),
                    Err(ScheduleTimeError::OutOfRange)
                ));
            }
        }

        let definition = ScheduleDefinition {
            scheduling: self::interval(interval, MissedTimePolicy::Skip),
            job_template: schedule(cron("0 * * * * UTC"), None).job_template,
            labels: vec![],
            time_range: TimeRange::default(),
        };
        assert!(matches!(
            validate_schedule(now, &definition),
            Err(ScheduleTimeError::OutOfRange)
        ));
    }

    #[test]
    fn invalid_cron_is_an_error() {
        let now = UNIX_EPOCH + Duration::from_secs(1000);

        let schedule = schedule(cron("not a cron"), None);
        assert!(matches!(
            next_execution_time(now, &schedule),
            Err(ScheduleTimeError::Cron(_))
        ));
    }
}
