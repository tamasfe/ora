use std::{future::Future, time::Duration};

use flume::Sender;
use futures::TryStreamExt;
use ora::{
    AdminClient, JobFilters, JobTypeId, ScheduleFilters, ScheduleOrderBy, ScheduleStatus,
    admin::schedules::StoppedScheduleJobAction, common::LabelFilter, execution::ExecutionStatus,
    executor::ExecutorId, job::JobId, schedule::ScheduleId,
};

use crate::tui::{AppEvent, events::Request};

/// The maximum number of items retrieved for any of the tables.
const LIST_LIMIT: u32 = 100;

/// How many of an executor's jobs are shown in its detail view.
const EXECUTOR_JOB_LIMIT: u32 = 25;

/// How long to wait for the server before giving up.
///
/// Without this a hung connection, such as a stale port forward,
/// leaves the view loading forever with nothing to explain it.
const REQUEST_TIMEOUT: Duration = Duration::from_mins(1);

/// Await a request, turning both failures and timeouts into a message
/// that can be shown in the footer.
async fn request<F, T>(what: &str, future: F) -> Result<T, String>
where
    F: Future<Output = ora::Result<T>>,
{
    match tokio::time::timeout(REQUEST_TIMEOUT, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(format!("{what}: {error}")),
        Err(_) => Err(format!(
            "{what}: no response after {}s",
            REQUEST_TIMEOUT.as_secs()
        )),
    }
}

pub(super) async fn update_job_types(admin: AdminClient, events: Sender<AppEvent>) {
    let event = match request("job types", admin.list_job_types()).await {
        Ok(job_types) => AppEvent::JobTypesUpdated(job_types),
        Err(error) => AppEvent::Failed(Request::JobTypes, error, None),
    };

    let _ = events.send_async(event).await;
}

pub(super) async fn update_jobs(
    token: u64,
    job_type: JobTypeId,
    order: ora::JobOrderBy,
    statuses: Option<Vec<ExecutionStatus>>,
    admin: AdminClient,
    events: Sender<AppEvent>,
    label_filter: String,
) {
    let filters = JobFilters {
        job_type_ids: Some(vec![job_type]),
        labels: parse_label_filter(&label_filter),
        execution_statuses: statuses,
        ..Default::default()
    };

    let event = match request(
        "jobs",
        admin
            .list_jobs(filters, order, Some(LIST_LIMIT))
            .try_collect::<Vec<_>>(),
    )
    .await
    {
        Ok(jobs) => AppEvent::JobsUpdated(
            token,
            jobs.into_iter().filter_map(ora::Job::into_raw).collect(),
        ),
        Err(error) => AppEvent::Failed(Request::Jobs, error, Some(token)),
    };

    let _ = events.send_async(event).await;
}

pub(super) async fn update_schedules(
    token: u64,
    job_type: JobTypeId,
    order: ScheduleOrderBy,
    statuses: Option<Vec<ScheduleStatus>>,
    admin: AdminClient,
    events: Sender<AppEvent>,
    label_filter: String,
) {
    let filters = ScheduleFilters {
        job_type_ids: Some(vec![job_type]),
        labels: parse_label_filter(&label_filter),
        statuses,
        ..Default::default()
    };

    let event = match request(
        "schedules",
        admin
            .list_schedules(filters, order, Some(LIST_LIMIT))
            .try_collect::<Vec<_>>(),
    )
    .await
    {
        Ok(schedules) => AppEvent::SchedulesUpdated(
            token,
            schedules
                .into_iter()
                .filter_map(ora::Schedule::into_raw)
                .collect(),
        ),
        Err(error) => AppEvent::Failed(Request::Schedules, error, Some(token)),
    };

    let _ = events.send_async(event).await;
}

pub(super) async fn update_executors(admin: AdminClient, events: Sender<AppEvent>) {
    let event = match request("executors", admin.list_executors()).await {
        Ok(executors) => AppEvent::ExecutorsUpdated(executors),
        Err(error) => AppEvent::Failed(Request::Executors, error, None),
    };

    let _ = events.send_async(event).await;
}

/// The most recent jobs handled by a single executor.
pub(super) async fn update_executor_jobs(
    executor_id: String,
    admin: AdminClient,
    events: Sender<AppEvent>,
) {
    let Ok(parsed) = executor_id.parse() else {
        let _ = events
            .send_async(AppEvent::Failed(
                Request::ExecutorJobs,
                format!("invalid executor ID: {executor_id}"),
                None,
            ))
            .await;
        return;
    };

    let filters = JobFilters {
        executor_ids: Some(vec![ExecutorId(parsed)]),
        ..Default::default()
    };

    let event = match request(
        "executor jobs",
        admin
            .list_jobs(
                filters,
                ora::JobOrderBy::CreatedAtDesc,
                Some(EXECUTOR_JOB_LIMIT),
            )
            .try_collect::<Vec<_>>(),
    )
    .await
    {
        Ok(jobs) => AppEvent::ExecutorJobsUpdated(
            executor_id,
            jobs.into_iter().filter_map(ora::Job::into_raw).collect(),
        ),
        Err(error) => AppEvent::Failed(Request::ExecutorJobs, error, None),
    };

    let _ = events.send_async(event).await;
}

pub(super) async fn cancel_job(job_id: String, admin: AdminClient, events: Sender<AppEvent>) {
    let filters = match job_id.parse() {
        Ok(id) => JobFilters {
            job_ids: Some(vec![JobId(id)]),
            ..Default::default()
        },
        Err(error) => {
            let _ = events
                .send_async(AppEvent::Failed(
                    Request::Action,
                    format!("invalid job ID: {error}"),
                    None,
                ))
                .await;
            return;
        }
    };

    let event = match request("cancel job", admin.cancel_jobs(filters)).await {
        Ok(_) => AppEvent::Refresh,
        Err(error) => AppEvent::Failed(Request::Action, error, None),
    };

    let _ = events.send_async(event).await;
}

pub(super) async fn stop_schedule(
    schedule_id: String,
    cancel_jobs: bool,
    admin: AdminClient,
    events: Sender<AppEvent>,
) {
    let filters = match schedule_id.parse() {
        Ok(id) => ScheduleFilters {
            schedule_ids: Some(vec![ScheduleId(id)]),
            ..Default::default()
        },
        Err(error) => {
            let _ = events
                .send_async(AppEvent::Failed(
                    Request::Action,
                    format!("invalid schedule ID: {error}"),
                    None,
                ))
                .await;
            return;
        }
    };

    let job_action = if cancel_jobs {
        StoppedScheduleJobAction::Cancel
    } else {
        StoppedScheduleJobAction::Ignore
    };

    let event = match request("stop schedule", admin.stop_schedules(filters, job_action)).await {
        Ok(_) => AppEvent::Refresh,
        Err(error) => AppEvent::Failed(Request::Action, error, None),
    };

    let _ = events.send_async(event).await;
}

/// Parse a comma-separated `key=value` filter as typed by the user,
/// a bare `key` matches any value.
fn parse_label_filter(filter: &str) -> Option<Vec<LabelFilter>> {
    if filter.trim().is_empty() {
        return None;
    }

    Some(
        filter
            .trim()
            .split(',')
            .filter_map(|s| {
                let mut s = s.trim().split('=');
                let key = s.next()?;
                let value = s.next();
                Some(LabelFilter {
                    key: key.to_string(),
                    value: value.map(ToString::to_string),
                })
            })
            .collect(),
    )
}
