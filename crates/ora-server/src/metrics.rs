use ahash::HashMap;
use futures::StreamExt;
use metrics::{counter, describe_counter, describe_gauge, gauge};
use ora_storage::{JobQueryFilters, ScheduleQueryFilters, Storage};

use crate::events::EventBus;

/// Collect and expose metrics of the server.
///
/// Note that the metrics collected here are not detailed
/// and are only meant to provide a high-level overview of
/// the server's activity.
pub(crate) async fn collect_metrics(store: &impl Storage, bus: &EventBus) -> eyre::Result<()> {
    let mut events = bus.subscribe_audit_events();

    describe_gauge!("ora_server_active_jobs", "Number of active jobs.");
    describe_counter!("ora_server_jobs_added_total", "Number of jobs added.");
    describe_counter!(
        "ora_server_jobs_cancelled_total",
        "Number of jobs cancelled."
    );
    describe_counter!("ora_server_jobs_deleted_total", "Number of jobs deleted.");
    describe_counter!(
        "ora_server_jobs_succeeded_total",
        "Number of jobs succeeded."
    );
    describe_counter!("ora_server_jobs_failed_total", "Number of jobs failed.");

    describe_gauge!(
        "ora_server_executors_connected",
        "Number of connected executors."
    );

    describe_counter!(
        "ora_server_snapshots_exported_total",
        "Number of snapshots exported."
    );
    describe_counter!(
        "ora_server_snapshots_imported_total",
        "Number of snapshots imported."
    );

    describe_counter!(
        "ora_server_schedules_added_total",
        "Number of schedules added."
    );

    describe_gauge!("ora_server_active_schedules", "Number of active schedules.");

    describe_counter!(
        "ora_server_schedules_cancelled_total",
        "Number of schedules cancelled."
    );

    describe_counter!(
        "ora_server_schedules_deleted_total",
        "Number of schedules deleted."
    );

    let active_job_count = store
        .count_jobs(JobQueryFilters {
            active: Some(true),
            ..Default::default()
        })
        .await?;

    #[allow(clippy::cast_precision_loss)]
    gauge!("ora_server_active_jobs").set(active_job_count as f64);

    let active_schedules_count = store
        .count_schedules(ScheduleQueryFilters {
            active: Some(true),
            ..Default::default()
        })
        .await?;

    #[allow(clippy::cast_precision_loss)]
    gauge!("ora_server_active_schedules").set(active_schedules_count as f64);

    let mut job_job_types = HashMap::default();
    let mut execution_jobs = HashMap::default();

    while let Some(event) = events.next().await {
        match event.kind {
            crate::AuditEventKind::JobAdded {
                job_id,
                job_type_id,
            } => {
                counter!(
                    "ora_server_jobs_added_total",
                    "job_type_id" => job_type_id.clone(),
                )
                .increment(1);

                gauge!("ora_server_active_jobs").increment(1);

                job_job_types.insert(job_id, job_type_id);
            }
            crate::AuditEventKind::JobCancelled { job_id } => {
                gauge!("ora_server_active_jobs").decrement(1);

                if let Some(job_type_id) = job_job_types.remove(&job_id) {
                    counter!(
                        "ora_server_jobs_cancelled_total",
                        "job_type_id" => job_type_id,
                    )
                    .increment(1);
                }
            }
            crate::AuditEventKind::JobDeleted { .. } => {
                counter!("ora_server_jobs_deleted_total").increment(1);
            }
            crate::AuditEventKind::ExecutionAdded {
                execution_id,
                job_id,
                ..
            } => {
                execution_jobs.insert(execution_id, job_id);
            }

            crate::AuditEventKind::ExecutionSucceeded { execution_id } => {
                gauge!("ora_server_active_jobs").decrement(1);

                if let Some(job_id) = execution_jobs.remove(&execution_id) {
                    if let Some(job_type_id) = job_job_types.remove(&job_id) {
                        counter!(
                            "ora_server_jobs_succeeded_total",
                            "job_type_id" => job_type_id,
                        )
                        .increment(1);
                    }
                }
            }
            crate::AuditEventKind::ExecutionFailed {
                execution_id,
                terminal,
            } => {
                if terminal {
                    gauge!("ora_server_active_jobs").decrement(1);

                    if let Some(job_id) = execution_jobs.remove(&execution_id) {
                        if let Some(job_type_id) = job_job_types.remove(&job_id) {
                            counter!(
                                "ora_server_jobs_failed_total",
                                "job_type_id" => job_type_id,
                            )
                            .increment(1);
                        }
                    }
                }
            }
            crate::AuditEventKind::ExecutorConnected { .. } => {
                gauge!("ora_server_executors_connected").increment(1);
            }
            crate::AuditEventKind::ExecutorDisconnected { .. } => {
                gauge!("ora_server_executors_connected").decrement(1);
            }
            crate::AuditEventKind::ScheduleAdded { .. } => {
                counter!("ora_server_schedules_added_total").increment(1);
                gauge!("ora_server_active_schedules").increment(1);
            }
            crate::AuditEventKind::ScheduleCancelled { .. } => {
                counter!("ora_server_schedules_cancelled_total").increment(1);
                gauge!("ora_server_active_schedules").decrement(1);
            }
            crate::AuditEventKind::ScheduleUnschedulable { .. } => {
                gauge!("ora_server_active_schedules").decrement(1);
            }
            crate::AuditEventKind::ScheduleDeleted { .. } => {
                counter!("ora_server_schedules_deleted_total").increment(1);
            }
            crate::AuditEventKind::SnapshotExported => {
                counter!("ora_server_snapshots_exported_total").increment(1);
            }
            crate::AuditEventKind::SnapshotImported => {
                counter!("ora_server_snapshots_imported_total").increment(1);
            }
            crate::AuditEventKind::ExecutionReady { .. }
            | crate::AuditEventKind::ExecutionAssigned { .. }
            | crate::AuditEventKind::ExecutionStarted { .. } => {}
        }
    }

    Ok(())
}
