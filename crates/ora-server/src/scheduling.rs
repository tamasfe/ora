use cronexpr::ParseOptions;
use eyre::Context;
use jiff::Timestamp;
use std::time::SystemTime;
use uuid::Uuid;

use crate::events::{EventBus, ExecutionEvent, JobEvent};

use ora_storage::{
    NewExecution, PendingExecution, PendingSchedule, ScheduleJobTimingPolicy,
    ScheduleMissedTimePolicy, Storage,
};

pub(crate) mod timer;

/// Schedule all pending jobs.
pub(crate) async fn create_executions(
    event_bus: &EventBus,
    backend: &impl Storage,
) -> eyre::Result<()> {
    let mut last_id = None;

    loop {
        let pending_jobs = backend.pending_jobs(last_id).await?;

        if pending_jobs.is_empty() {
            break;
        }

        last_id = pending_jobs.iter().map(|j| j.id).max();

        let now = SystemTime::now();
        let mut new_executions = Vec::with_capacity(pending_jobs.len());
        let mut completed_jobs = Vec::new();

        for pending_job in pending_jobs {
            if pending_job.execution_count >= 1
                && pending_job.execution_count > pending_job.retry_policy.retries
            {
                completed_jobs.push(pending_job.id);
                continue;
            }

            let execution_id = Uuid::now_v7();

            new_executions.push(NewExecution {
                id: execution_id,
                job_id: pending_job.id,
                attempt_number: pending_job.execution_count.saturating_add(1),
                target_execution_time: pending_job.target_execution_time,
            });
        }

        let execution_count = new_executions.len();

        backend.executions_added(new_executions, now).await?;
        backend.jobs_unschedulable(&completed_jobs, now).await?;

        tracing::trace!(execution_count, "created new executions",);

        tracing::trace!(
            job_count = completed_jobs.len(),
            "inactivated completed jobs",
        );

        event_bus.emit_execution_event(ExecutionEvent::ExecutionsAdded);
    }

    Ok(())
}

/// Schedule all pending executions, returning the last ID.
pub(crate) async fn schedule_executions(
    backend: &impl Storage,
    to_timer: &mut rtrb::Producer<PendingExecution>,
    last_id: Option<Uuid>,
) -> eyre::Result<Option<Uuid>> {
    let executions = backend.pending_executions(last_id).await?;
    let last_id = executions.iter().map(|e| e.id).max();

    let mut executions = executions.into_iter();

    if to_timer.slots() == 0 {
        tracing::warn!("buffer is full, consider increasing the buffer size");
    }

    loop {
        let Ok(ready_buf) = to_timer.write_chunk_uninit(to_timer.slots()) else {
            // The buffer is full, we need to wait for the timer to catch up.
            continue;
        };

        ready_buf.fill_from_iter(&mut executions);

        if executions.len() == 0 {
            break;
        }
    }

    Ok(last_id)
}

/// Mark executions as ready.
pub(crate) async fn mark_executions_ready(
    event_bus: &EventBus,
    backend: &impl Storage,
    ready_executions: &mut rtrb::Consumer<Uuid>,
) -> eyre::Result<()> {
    let now = SystemTime::now();

    let buf = ready_executions
        .read_chunk(ready_executions.slots())
        .unwrap();

    let (s1, s2) = buf.as_slices();

    tracing::trace!(
        execution_count = s1.len() + s2.len(),
        "marked executions as ready",
    );

    backend.executions_ready(s1, now).await?;
    backend.executions_ready(s2, now).await?;

    event_bus.emit_execution_event(ExecutionEvent::ExecutionsReadyToRun);

    buf.commit_all();

    Ok(())
}

/// Create new jobs for pending schedules.
pub(crate) async fn create_schedule_jobs(
    event_bus: &EventBus,
    backend: &impl Storage,
) -> eyre::Result<()> {
    let mut last_id = None;

    loop {
        let pending_schedules = backend.pending_schedules(last_id).await?;

        if pending_schedules.is_empty() {
            break;
        }

        last_id = pending_schedules.iter().map(|s| s.id).max();

        let now = SystemTime::now();
        let mut new_jobs = Vec::with_capacity(pending_schedules.len());
        let mut schedules_to_deactivate = Vec::new();

        for pending_schedule in pending_schedules {
            let target_execution_time = match next_execution_time(now, &pending_schedule) {
                Ok(exec_time) => exec_time,
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        schedule_id = %pending_schedule.id,
                        "failed to calculate next execution time, marking schedule as inactive",
                    );

                    schedules_to_deactivate.push(pending_schedule.id);
                    continue;
                }
            };

            if let Some(time_range) = pending_schedule.time_range {
                if let Some(after) = time_range.start {
                    if target_execution_time < after {
                        tracing::debug!(
                            schedule_id = %pending_schedule.id,
                            "schedule is not ready to run yet",
                        );
                        continue;
                    }
                }

                if let Some(before) = time_range.end {
                    if target_execution_time > before {
                        tracing::debug!(
                            schedule_id = %pending_schedule.id,
                            "schedule is past its end time, marking as inactive",
                        );

                        schedules_to_deactivate.push(pending_schedule.id);
                        continue;
                    }
                }
            }

            match pending_schedule.job_creation_policy {
                ora_storage::ScheduleJobCreationPolicy::JobDefinition(
                    schedule_new_job_definition,
                ) => {
                    let job_id = Uuid::now_v7();
                    new_jobs.push(ora_storage::NewJob {
                        id: job_id,
                        schedule_id: Some(pending_schedule.id),
                        created_at: now,
                        job_type_id: schedule_new_job_definition.job_type_id,
                        target_execution_time,
                        input_payload_json: schedule_new_job_definition.input_payload_json,
                        timeout_policy: schedule_new_job_definition.timeout_policy,
                        retry_policy: schedule_new_job_definition.retry_policy,
                        labels: schedule_new_job_definition.labels,
                        metadata_json: None,
                    });
                }
            }
        }

        if !new_jobs.is_empty() {
            backend
                .jobs_added(new_jobs)
                .await
                .wrap_err("failed to create schedule jobs")?;

            event_bus.emit_job_event(JobEvent::JobsCreated);
        }

        if !schedules_to_deactivate.is_empty() {
            backend
                .schedules_unschedulable(&schedules_to_deactivate, now)
                .await
                .wrap_err("failed to mark schedules as inactive")?;
        }
    }

    Ok(())
}

fn next_execution_time(now: SystemTime, schedule: &PendingSchedule) -> eyre::Result<SystemTime> {
    match &schedule.job_timing_policy {
        ScheduleJobTimingPolicy::Repeat(policy) => {
            if let Some(last_time) = schedule.last_target_execution_time {
                match policy.missed_policy {
                    ScheduleMissedTimePolicy::Skip => {
                        let mut next_time = last_time + policy.interval;

                        while next_time < now {
                            next_time += policy.interval;
                        }

                        Ok(next_time)
                    }
                    ScheduleMissedTimePolicy::Create => Ok(last_time + policy.interval),
                }
            } else if policy.immediate {
                Ok(now)
            } else {
                Ok(now + policy.interval)
            }
        }
        ScheduleJobTimingPolicy::Cron(policy) => {
            let now = Timestamp::try_from(now).unwrap();

            let mut cron_parse_options = ParseOptions::default();
            cron_parse_options.fallback_timezone_option = cronexpr::FallbackTimezoneOption::UTC;

            let cron = cronexpr::parse_crontab_with(&policy.cron_expression, cron_parse_options)
                .wrap_err("invalid cron expression")?;

            if let Some(last_time) = schedule.last_target_execution_time {
                let last_time = Timestamp::try_from(last_time).unwrap();

                let mut next_time = cron
                    .find_next(last_time)
                    .wrap_err("failed to calculate next execution time")?
                    .timestamp();

                while next_time < now {
                    next_time = cron
                        .find_next(next_time)
                        .wrap_err("failed to calculate next execution time")?
                        .timestamp();
                }

                Ok(next_time.into())
            } else if policy.immediate {
                Ok(now.into())
            } else {
                let next_time = cron
                    .find_next(now)
                    .wrap_err("failed to calculate next execution time")?;

                Ok(next_time.timestamp().into())
            }
        }
    }
}
