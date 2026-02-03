use futures::StreamExt;
use jiff::Timestamp;
use std::{pin::pin, sync::Arc, time::SystemTime};
use wgroup::WaitGuard;

use ora_backend::{
    Backend,
    jobs::{JobDefinition, NewJob},
    schedules::{MissedTimePolicy, PendingSchedule, ScheduleFilters, SchedulingPolicy},
};

#[tracing::instrument(skip_all)]
pub(super) async fn schedule_new_jobs_loop(backend: Arc<impl Backend>, wg: WaitGuard) {
    loop {
        let mut stream = pin!(backend.pending_schedules());

        while let Some(pending_schedules) = stream.next().await {
            let pending_schedules = match pending_schedules {
                Ok(s) => s,
                Err(error) => {
                    tracing::error!(%error, "failed to retrieve pending schedules");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    break;
                }
            };

            let now = SystemTime::now();

            let mut new_jobs = Vec::with_capacity(pending_schedules.len());
            let mut stop_scheduling_ids = Vec::new();

            for schedule in pending_schedules {
                let next_time = match next_execution_time(now, &schedule) {
                    Ok(s) => s,
                    Err(error) => {
                        tracing::error!(
                            %error,
                            schedule_id = %schedule.schedule_id,
                            "failed to determine execution time for schedule",
                        );
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
                        tracing::debug!(job_count = jobs.len(), "spawned new jobs for schedules");
                    }
                    Err(error) => {
                        tracing::error!(%error, "failed to spawn jobs for schedules");
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
            _ = backend.wait_for_pending_schedules() => {}
            _ = check_delay => {
                tracing::trace!("periodic check for pending schedules");
            }
        }
    }
}

fn next_execution_time(
    mut now: SystemTime,
    schedule: &PendingSchedule,
) -> Result<SystemTime, cronexpr::Error> {
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
            if let Some(last_time) = schedule.last_target_execution_time {
                match missed {
                    MissedTimePolicy::Skip => {
                        let mut next_time = last_time + *interval;

                        while next_time < now {
                            next_time += *interval;
                        }

                        Ok(next_time)
                    }
                    MissedTimePolicy::Create => Ok(last_time + *interval),
                }
            } else if *immediate {
                Ok(now)
            } else {
                Ok(now + *interval)
            }
        }
        SchedulingPolicy::Cron {
            expression,
            immediate,
            missed,
        } => {
            let now = Timestamp::try_from(now).unwrap();

            let mut cron_parse_options = cronexpr::ParseOptions::default();
            cron_parse_options.fallback_timezone_option = cronexpr::FallbackTimezoneOption::System;

            let cron = cronexpr::parse_crontab_with(expression, cron_parse_options)?;

            if let Some(last_time) = schedule.last_target_execution_time {
                let last_time = Timestamp::try_from(last_time).unwrap();

                let next_time = match missed {
                    MissedTimePolicy::Skip => cron.find_next(now)?.timestamp(),
                    MissedTimePolicy::Create => cron.find_next(last_time)?.timestamp(),
                };

                Ok(next_time.into())
            } else if *immediate {
                Ok(now.into())
            } else {
                let next_time = cron.find_next(now)?;
                Ok(next_time.timestamp().into())
            }
        }
    }
}
