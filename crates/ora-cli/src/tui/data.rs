use flume::Sender;
use futures::TryStreamExt;
use ora::{AdminClient, JobFilters, JobTypeId, common::LabelFilter};

use crate::tui::AppEvent;

pub(super) async fn update_job_types(admin: AdminClient, events: Sender<AppEvent>) {
    if let Ok(job_types) = admin.list_job_types().await {
        let _ = events
            .send_async(AppEvent::JobTypesUpdated(job_types))
            .await;
    }
}

pub(super) async fn update_jobs(
    job_type: Option<JobTypeId>,
    order: ora::JobOrderBy,
    active_only: bool,
    admin: AdminClient,
    events: Sender<AppEvent>,
    label_filter: String,
) {
    let Some(job_type) = job_type else {
        let _ = events.send_async(AppEvent::JobsUpdated(vec![])).await;
        return;
    };

    let mut filters = JobFilters {
        job_type_ids: Some(vec![job_type]),
        ..Default::default()
    };

    if !label_filter.is_empty() {
        filters.labels = Some(
            label_filter
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
        );
    }

    if active_only {
        filters = filters.active_only();
    }

    if let Ok(jobs) = admin
        .list_jobs(filters, order, Some(100))
        .try_collect::<Vec<_>>()
        .await
    {
        let _ = events
            .send_async(AppEvent::JobsUpdated(
                jobs.into_iter().filter_map(ora::Job::into_raw).collect(),
            ))
            .await;
    }
}
