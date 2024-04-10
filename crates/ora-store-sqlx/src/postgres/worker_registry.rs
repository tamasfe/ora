#![allow(clippy::module_name_repetitions)]
use std::collections::BTreeMap;

use async_trait::async_trait;
use ora_worker::registry::{
    HeartbeatData, HeartbeatResponse, SupportedTask, WorkerInfo, WorkerMetadata, WorkerRegistry,
};
use serde_json::Value;
use sqlx::{query, query_as, types::Json, FromRow, Postgres};
use time::OffsetDateTime;

use super::DbStore;

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
