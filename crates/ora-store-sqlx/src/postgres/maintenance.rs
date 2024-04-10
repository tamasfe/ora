//! Maintenance tasks.

use sqlx::{query, query_as, Postgres};
use uuid::Uuid;

use crate::DbStore;

/// Options for maintenance tasks.
#[derive(Debug, Clone)]
pub struct MaintenanceOptions {
    /// Worker registry maintenance options.
    pub worker_registry: WorkerRegistryMaintenanceOptions,
    /// Task maintenance options.
    pub task: TaskMaintenanceOptions,
    /// Schedule maintenance options.
    pub schedule: ScheduleMaintenanceOptions,
}

/// Worker registry maintenance options.
#[derive(Debug, Clone)]
pub struct WorkerRegistryMaintenanceOptions {
    /// Mark workers as inactive after this duration.
    ///
    /// Inactive workers are considered to be dead and they are
    /// not expected to reappear.
    /// 
    /// Always make sure that the heartbeat interval of workers is 
    /// less than this duration.
    pub worker_inactive_after: time::Duration,
    /// Remove inactive workers after this duration from the registry.
    pub remove_inactive_workers_after: time::Duration,
    /// Fail running tasks of a worker once it is marked as inactive.
    pub fail_inactive_worker_tasks: bool,
}

impl Default for WorkerRegistryMaintenanceOptions {
    fn default() -> Self {
        Self {
            worker_inactive_after: time::Duration::minutes(5),
            remove_inactive_workers_after: time::Duration::minutes(60),
            fail_inactive_worker_tasks: true,
        }
    }
}

/// Task maintenance options.
#[derive(Debug, Default, Clone)]
pub struct TaskMaintenanceOptions {
    /// Remove inactive tasks after this duration.
    pub remove_inactive_tasks_after: Option<time::Duration>,
}

/// Schedule maintenance options.
#[derive(Debug, Default, Clone)]
pub struct ScheduleMaintenanceOptions {
    /// Remove inactive schedules after this duration.
    pub remove_inactive_schedules_after: Option<time::Duration>,
}

impl DbStore<Postgres> {
    /// Run all maintenance tasks.
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn run_all_maintenance(&self, options: &MaintenanceOptions) -> eyre::Result<()> {
        self.run_worker_registry_maintenance(&options.worker_registry)
            .await?;
        self.run_task_maintenance(&options.task).await?;
        self.run_schedule_maintenance(&options.schedule).await?;
        Ok(())
    }

    /// Perform maintenance on tasks.
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn run_task_maintenance(&self, options: &TaskMaintenanceOptions) -> eyre::Result<()> {
        if let Some(remove_after) = options.remove_inactive_tasks_after {
            query(
                r#"--sql
                DELETE FROM "ora"."task"
                WHERE
                    NOT "active"
                    AND "updated" < NOW() - $1::INTERVAL
                "#,
            )
            .bind(remove_after)
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }

    /// Perform maintenance on the schedules.
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn run_schedule_maintenance(
        &self,
        options: &ScheduleMaintenanceOptions,
    ) -> eyre::Result<()> {
        if let Some(remove_after) = options.remove_inactive_schedules_after {
            query(
                r#"--sql
                DELETE FROM "ora"."schedule"
                WHERE
                    NOT "active"
                    AND "updated" < NOW() - $1::INTERVAL
                "#,
            )
            .bind(remove_after)
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }

    /// Perform maintenance on the worker registry.
    ///
    /// This will mark workers as inactive and remove inactive workers.
    ///
    /// # Errors
    ///
    /// This will return an error if the database operation fails.
    pub async fn run_worker_registry_maintenance(
        &self,
        options: &WorkerRegistryMaintenanceOptions,
    ) -> Result<(), sqlx::Error> {
        let worker_id_rows: Vec<(Uuid,)> = query_as(
            r#"--sql
            UPDATE "ora"."worker"
            SET
                "active" = FALSE
            WHERE 
                "active" = TRUE
                AND "last_seen" < NOW() - $1::INTERVAL
            RETURNING "id"
            "#,
        )
        .bind(options.worker_inactive_after)
        .fetch_all(&self.db)
        .await?;

        if options.fail_inactive_worker_tasks {
            let worker_ids = worker_id_rows
                .into_iter()
                .map(|(id,)| id)
                .collect::<Vec<_>>();

            query(
                r#"--sql
                UPDATE "ora"."task"
                SET
                    "status" = 'failed',
                    "failure_reason" = 'worker has became inactive',
                    "failed_at" = NOW()
                WHERE "worker_id" = ANY($1) AND "active"
                RETURNING pg_notify('ora_task_failed', "id"::TEXT) AS "notified";
                "#,
            )
            .bind(worker_ids)
            .execute(&self.db)
            .await?;
        }

        query(
            r#"--sql
            DELETE FROM "ora"."worker"
            WHERE
                "active" = FALSE
                AND "last_seen" < NOW() - $1::INTERVAL
            "#,
        )
        .bind(options.remove_inactive_workers_after)
        .execute(&self.db)
        .await?;

        Ok(())
    }
}
