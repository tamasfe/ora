#![allow(clippy::module_name_repetitions)]
use std::collections::BTreeMap;

use async_trait::async_trait;
use ora_worker::registry::{
    HeartbeatData, HeartbeatResponse, SupportedTask, WorkerInfo, WorkerMetadata, WorkerRegistry,
};
use serde_json::Value;
use sqlx::{query, query_as, types::Json, FromRow, Postgres};
use time::OffsetDateTime;
use uuid::Uuid;

use super::DbStore;

/// Worker registry maintenance options.
#[derive(Debug, Clone)]
pub struct WorkerRegistryMaintenanceOptions {
    /// Mark workers as inactive after this duration.
    ///
    /// Inactive workers are considered to be offline and they are
    /// not expected to reappear.
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

impl DbStore<Postgres> {
    /// Perform maintenance on the worker registry.
    ///
    /// This will mark workers as inactive and remove inactive workers.
    ///
    /// # Errors
    ///
    /// This will return an error if the database operation fails.
    pub async fn worker_registry_maintenance(
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
            let worker_ids = worker_id_rows.into_iter().map(|(id,)| id).collect::<Vec<_>>();

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

#[async_trait]
impl WorkerRegistry for DbStore<Postgres> {
    type Error = sqlx::Error;

    async fn register_worker(
        &self,
        worker_id: uuid::Uuid,
        metadata: &ora_worker::registry::WorkerMetadata,
    ) -> Result<(), Self::Error> {
        query(
            r#"--sql
            INSERT INTO "ora"."worker" (
                "id",
                "active",
                "name",
                "description",
                "version",
                "supported_tasks",
                "other_metadata"
            ) VALUES (
                $1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7
            )
            ON CONFLICT ("id") DO UPDATE SET
                "active" = EXCLUDED."active",
                "name" = EXCLUDED."name",
                "description" = EXCLUDED."description",
                "version" = EXCLUDED."version",
                "supported_tasks" = EXCLUDED."supported_tasks",
                "other_metadata" = EXCLUDED."other_metadata",
                "updated" = NOW(),
                "last_seen" = NOW()
            "#,
        )
        .bind(worker_id)
        .bind(true)
        .bind(&metadata.name)
        .bind(&metadata.description)
        .bind(&metadata.version)
        .bind(Json(&metadata.supported_tasks))
        .bind(Json(&metadata.other))
        .execute(&self.db)
        .await?;

        Ok(())
    }

    async fn unregister_worker(&self, worker_id: uuid::Uuid) -> Result<(), Self::Error> {
        query(
            r#"--sql
            UPDATE
                "ora"."worker"
            SET
                "updated" = NOW(),
                "active" = FALSE
            WHERE
                "id" = $1
            "#,
        )
        .bind(worker_id)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    async fn heartbeat(
        &self,
        worker_id: uuid::Uuid,
        _data: &HeartbeatData,
    ) -> Result<HeartbeatResponse, Self::Error> {
        let registered = query(
            r#"--sql
            UPDATE
                "ora"."worker"
            SET
                "active" = TRUE,
                "updated" = NOW(),
                "last_seen" = NOW()
            WHERE
                "id" = $1
            RETURNING 1 AS "registered"
            "#,
        )
        .bind(worker_id)
        .fetch_optional(&self.db)
        .await?
        .is_some();

        Ok(HeartbeatResponse {
            should_register: !registered,
        })
    }

    async fn workers(&self) -> Result<Vec<WorkerInfo>, Self::Error> {
        #[derive(FromRow)]
        struct WorkerRow {
            id: uuid::Uuid,
            name: Option<String>,
            description: Option<String>,
            version: Option<String>,
            supported_tasks: Json<Vec<SupportedTask>>,
            other_metadata: Json<BTreeMap<String, Value>>,
            created: OffsetDateTime,
            last_seen: OffsetDateTime,
        }

        let workers: Vec<WorkerRow> = query_as(
            r#"--sql
            SELECT
                "id",
                "name",
                "description",
                "version",
                "supported_tasks",
                "other_metadata",
                "created",
                "last_seen"
            FROM
                "ora"."worker"
            WHERE
                "active" = TRUE
            "#,
        )
        .fetch_all(&self.db)
        .await?;

        Ok(workers
            .into_iter()
            .map(|row| WorkerInfo {
                id: row.id,
                metadata: WorkerMetadata {
                    name: row.name,
                    description: row.description,
                    version: row.version,
                    supported_tasks: row.supported_tasks.0,
                    other: row.other_metadata.0,
                },
                registered: row.created,
                last_seen: row.last_seen,
            })
            .collect())
    }
}
