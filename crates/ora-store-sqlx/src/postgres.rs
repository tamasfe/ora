//! Postgres backend implementation.
#![allow(clippy::cast_sign_loss)]

use std::{
    collections::HashSet,
    marker::PhantomData,
    sync::{atomic::Ordering, Arc},
};

use super::DbStore;
use crate::{
    models::{DbEvent, Ora, Schedule, Task},
    DbStoreOptions,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use ora_client::{
    ClientOperations, RawTaskResult, ScheduleListOrder, ScheduleOperations, Schedules,
    TaskListOrder, TaskOperations, Tasks,
};
use ora_common::{
    schedule::ScheduleDefinition,
    task::{TaskDataFormat, TaskDefinition, TaskStatus, WorkerSelector},
};
use ora_scheduler::store::{
    schedule::{ActiveSchedule, SchedulerScheduleStore, SchedulerScheduleStoreEvent},
    task::{ActiveTask, PendingTask, SchedulerTaskStore, SchedulerTaskStoreEvent},
};
use ora_worker::store::{ReadyTask, WorkerStore, WorkerStoreEvent};
use sea_query::{Alias, Condition, Expr, IntoCondition, Order, PostgresQueryBuilder, Query};
use sea_query_binder::SqlxValues;
use serde_json::{value::RawValue, Value};
use sqlx::{
    postgres::PgRow,
    query, query_as, query_as_with, query_with,
    types::{time::OffsetDateTime, Json},
    Connection, Executor, PgPool, Postgres, Row,
};
use tap::TapFallible;
use tokio::sync::broadcast::{self, error::RecvError};
use uuid::Uuid;

mod events;
pub mod maintenance;
mod migrations;
mod worker_registry;

pub use maintenance::{
    MaintenanceOptions, ScheduleMaintenanceOptions, TaskMaintenanceOptions,
    WorkerRegistryMaintenanceOptions,
};

impl DbStore<Postgres> {
    /// Create a new store backed by the given pool.
    ///
    /// *note*: A connection is always reserved for watching events,
    /// make sure that there are at least two connections allowed in the pool.
    ///
    /// # Errors
    ///
    /// Errors are returned if migrations fail to apply
    /// or in case any other database error occurs.
    pub async fn new(db: PgPool) -> eyre::Result<Self> {
        Self::new_with_options(
            db,
            DbStoreOptions {
                poll_interval: time::Duration::hours(1),
                ..Default::default()
            },
        )
        .await
    }

    /// Create a new store backed by the given pool and options.
    ///
    /// *note*: A connection is always reserved for watching events,
    /// make sure that there are at least two connections allowed in the pool.
    ///
    /// # Errors
    ///
    /// Errors are returned if migrations fail to apply
    /// or in case any other database error occurs.
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn new_with_options(db: PgPool, options: DbStoreOptions) -> eyre::Result<Self> {
        let (events, _) = tokio::sync::broadcast::channel(options.channel_capacity);
        let this = Self {
            db,
            options,
            events,
            worker_selector_version: Arc::default(),
            worker_selectors: Arc::default(),
            store_count: Arc::new(()),
        };
        this.migrate().await?;
        this.setup_poll_loop().await?;
        Ok(this)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn migrate(&self) -> eyre::Result<()> {
        // We store the migrations table inside the schema as well,
        // so we need to make sure the schema exists even before migrations
        // are applied.
        self.db
            .execute(
                r#"--sql
                CREATE SCHEMA IF NOT EXISTS "ora";
                "#,
            )
            .await?;

        let mut migrator = sqlx_migrate::Migrator::connect_with_pool(&self.db).await?;
        migrator.add_migrations(migrations::migrations());
        migrator.set_migrations_table(r#""ora"."migrations""#);
        migrator.migrate_all().await?;
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn add_task_with(
        &self,
        mut task: TaskDefinition,
        schedule_id: Option<Uuid>,
    ) -> sqlx::Result<Uuid> {
        let task_id = Uuid::new_v4();

        let (sql, values) = Query::insert()
            .into_table((Ora, Task::Table))
            .columns([
                Task::Id,
                Task::Status,
                Task::ScheduleId,
                Task::Target,
                Task::WorkerSelector,
                Task::DataBytes,
                Task::DataJson,
                Task::DataFormat,
                Task::Labels,
                Task::TimeoutPolicy,
            ])
            .values([
                // Task::Id
                task_id.into(),
                // Task::Status
                TaskStatus::Pending.as_str().into(),
                // Task::ScheduleId
                schedule_id.into(),
                // Task::Target
                OffsetDateTime::from(task.target).into(),
                // Task::WorkerSelector
                serde_json::to_value(task.worker_selector).unwrap().into(),
                // Task::DataBytes
                match task.data_format {
                    TaskDataFormat::Unknown | TaskDataFormat::MessagePack => {
                        Some(std::mem::take(&mut task.data)).into()
                    }
                    TaskDataFormat::Json => Option::<Vec<u8>>::None.into(),
                },
                // Task::DataJson
                match task.data_format {
                    TaskDataFormat::Unknown | TaskDataFormat::MessagePack => {
                        Option::<Value>::None.into()
                    }
                    TaskDataFormat::Json => Some(
                        serde_json::from_slice::<Value>(&task.data)
                            .tap_err(|error| {
                                tracing::error!(
                                %error,
                                "invalid task JSON data");
                            })
                            .unwrap_or_default(),
                    )
                    .into(),
                },
                // Task::DataFormat
                task.data_format.as_str().into(),
                // Task::Labels
                serde_json::to_value(task.labels).unwrap().into(),
                // Task::TimeoutPolicy
                serde_json::to_value(task.timeout).unwrap().into(),
            ])
            .unwrap()
            .build(PostgresQueryBuilder);

        sqlx::query_with(&sql, sea_query_binder::SqlxValues(values))
            .execute(&self.db)
            .await?;

        Ok(task_id)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    fn update_worker_selectors(&self, selectors: &[WorkerSelector]) {
        let mut global_ws = self.worker_selectors.lock().unwrap();
        let mut selectors_changed = false;
        for selector in selectors {
            if !global_ws.contains(selector) {
                global_ws.insert(selector.clone());
                selectors_changed = true;
            }
        }
        drop(global_ws);
        if selectors_changed {
            self.worker_selector_version.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn task_definition_from_row(row: PgRow) -> Result<TaskDefinition, sqlx::Error> {
    let data_format: TaskDataFormat = row.try_get::<&str, _>("data_format")?.parse().unwrap();
    Ok(TaskDefinition {
        target: row.try_get::<OffsetDateTime, _>("target")?.into(),
        worker_selector: row.try_get::<Json<_>, _>("worker_selector")?.0,
        data: match data_format {
            TaskDataFormat::Unknown | TaskDataFormat::MessagePack => {
                row.try_get::<Vec<u8>, _>("data_bytes")?
            }
            TaskDataFormat::Json => row
                .try_get::<Json<&RawValue>, _>("data_json")?
                .0
                .get()
                .as_bytes()
                .to_vec(),
        },
        data_format,
        labels: row.try_get::<Json<_>, _>("labels")?.0,
        timeout: row.try_get::<Json<_>, _>("timeout_policy")?.0,
        _task_type: PhantomData,
    })
}

#[async_trait]
impl ClientOperations for DbStore<Postgres> {
    #[tracing::instrument(level = "debug", skip_all)]
    async fn add_task(&self, task: TaskDefinition) -> eyre::Result<Uuid> {
        self.add_task_with(task, None).await.map_err(Into::into)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task(&self, task_id: Uuid) -> eyre::Result<Arc<dyn TaskOperations>> {
        query(
            r#"--sql
            SELECT 1 as "exists" FROM "ora"."task"
            WHERE
                "id" = $1
            "#,
        )
        .bind(task_id)
        .fetch_one(&self.db)
        .await?;
        Ok(Arc::new(PgTaskOperations {
            task_id,
            events: self.events.clone(),
            db: self.db.clone(),
        }))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn tasks(&self, options: &Tasks) -> eyre::Result<Vec<Arc<dyn TaskOperations>>> {
        let mut q = Query::select();
        q.column(Task::Id)
            .from((Ora, Task::Table))
            .cond_where(filter_tasks(options))
            .offset(options.offset)
            .limit(u64::min(options.limit, i64::MAX as u64));

        match options.order {
            TaskListOrder::AddedAsc => q.order_by(Task::AddedAt, Order::Asc),
            TaskListOrder::AddedDesc => q.order_by(Task::AddedAt, Order::Desc),
            TaskListOrder::TargetAsc => q.order_by(Task::Target, Order::Asc),
            TaskListOrder::TargetDesc => q.order_by(Task::Target, Order::Desc),
        };

        let (sql, values) = q.build(PostgresQueryBuilder);

        Ok(query_with(&sql, SqlxValues(values))
            .try_map(|row: PgRow| {
                Ok(Arc::new(PgTaskOperations {
                    task_id: row.try_get(0)?,
                    events: self.events.clone(),
                    db: self.db.clone(),
                }) as Arc<_>)
            })
            .fetch_all(&self.db)
            .await?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_count(&self, options: &Tasks) -> eyre::Result<u64> {
        let (sql, values) = Query::select()
            .expr(Expr::col(Task::Id).count())
            .from((Ora, Task::Table))
            .cond_where(filter_tasks(options))
            .build(PostgresQueryBuilder);

        let res: (i64,) = query_as_with(&sql, SqlxValues(values))
            .fetch_one(&self.db)
            .await?;

        Ok(res.0 as _)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_labels(&self) -> eyre::Result<Vec<String>> {
        let labels: Vec<String> = query(
            r#"--sql
            SELECT DISTINCT UNNEST(json_object_keys("labels")) AS "label" FROM "ora"."task"
            "#,
        )
        .try_map(|row: PgRow| row.try_get(0))
        .fetch_all(&self.db)
        .await?;

        Ok(labels)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_kinds(&self) -> eyre::Result<Vec<String>> {
        let kinds: Vec<String> = query(
            r#"--sql
            SELECT DISTINCT ('worker_selector' ->> 'kind') AS "kind" FROM "ora"."task"
            "#,
        )
        .try_map(|row: PgRow| row.try_get(0))
        .fetch_all(&self.db)
        .await?;

        Ok(kinds)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn add_schedule(&self, schedule: ScheduleDefinition) -> eyre::Result<Uuid> {
        let schedule_id = Uuid::new_v4();

        let (sql, values) = Query::insert()
            .into_table((Ora, Schedule::Table))
            .columns([
                Schedule::Id,
                Schedule::SchedulePolicy,
                Schedule::Immediate,
                Schedule::MissedTasksPolicy,
                Schedule::NewTask,
                Schedule::Labels,
            ])
            .values([
                // Schedule::Id,
                schedule_id.into(),
                // Schedule::SchedulePolicy,
                serde_json::to_value(schedule.policy).unwrap().into(),
                // Schedule::Immediate,
                schedule.immediate.into(),
                // Schedule::MissedTasksPolicy,
                serde_json::to_value(schedule.missed_tasks).unwrap().into(),
                // Schedule::NewTask,
                serde_json::to_value(schedule.new_task).unwrap().into(),
                // Schedule::Labels,
                serde_json::to_value(schedule.labels).unwrap().into(),
            ])?
            .build(PostgresQueryBuilder);

        sqlx::query_with(&sql, sea_query_binder::SqlxValues(values))
            .execute(&self.db)
            .await?;

        Ok(schedule_id)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn schedule(
        &self,
        schedule_id: Uuid,
    ) -> eyre::Result<Arc<dyn ora_client::ScheduleOperations>> {
        query(
            r#"--sql
            SELECT 1 as "exists" FROM "ora"."schedule"
            WHERE
                "id" = $1
            "#,
        )
        .bind(schedule_id)
        .fetch_one(&self.db)
        .await?;
        Ok(Arc::new(PgScheduleOperations {
            schedule_id,
            events: self.events.clone(),
            db: self.db.clone(),
        }))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn schedules(
        &self,
        options: &Schedules,
    ) -> eyre::Result<Vec<Arc<dyn ora_client::ScheduleOperations>>> {
        let mut q = Query::select();
        q.column(Schedule::Id)
            .from((Ora, Schedule::Table))
            .cond_where(filter_schedules(options))
            .offset(options.offset)
            .limit(u64::min(options.limit, i64::MAX as u64));

        match options.order {
            ScheduleListOrder::AddedAsc => q.order_by(Schedule::AddedAt, Order::Asc),
            ScheduleListOrder::AddedDesc => q.order_by(Schedule::AddedAt, Order::Desc),
        };

        let (sql, values) = q.build(PostgresQueryBuilder);

        Ok(query_with(&sql, SqlxValues(values))
            .try_map(|row: PgRow| {
                Ok(Arc::new(PgScheduleOperations {
                    schedule_id: row.try_get(0)?,
                    events: self.events.clone(),
                    db: self.db.clone(),
                }) as Arc<_>)
            })
            .fetch_all(&self.db)
            .await?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn schedule_count(&self, options: &Schedules) -> eyre::Result<u64> {
        let (sql, values) = Query::select()
            .expr(Expr::col(Schedule::Id).count())
            .from((Ora, Schedule::Table))
            .cond_where(filter_schedules(options))
            .build(PostgresQueryBuilder);

        let res: (i64,) = query_as_with(&sql, SqlxValues(values))
            .fetch_one(&self.db)
            .await?;

        Ok(res.0 as _)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn schedule_labels(&self) -> eyre::Result<Vec<String>> {
        let labels: Vec<String> = query(
            r#"--sql
            SELECT DISTINCT UNNEST(json_object_keys("labels")) AS "label" FROM "ora"."schedule"
            "#,
        )
        .try_map(|row: PgRow| row.try_get(0))
        .fetch_all(&self.db)
        .await?;

        Ok(labels)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn add_tasks(
        &self,
        tasks: &mut (dyn ExactSizeIterator<Item = TaskDefinition> + Send),
    ) -> eyre::Result<Vec<Uuid>> {
        let mut tx = self.db.begin().await?;

        let mut id = Vec::with_capacity(tasks.len());
        let mut status = Vec::with_capacity(tasks.len());
        let mut schedule_id = Vec::with_capacity(tasks.len());
        let mut target = Vec::with_capacity(tasks.len());
        let mut worker_selector = Vec::with_capacity(tasks.len());
        let mut data_bytes = Vec::with_capacity(tasks.len());
        let mut data_json = Vec::with_capacity(tasks.len());
        let mut data_format = Vec::with_capacity(tasks.len());
        let mut labels = Vec::with_capacity(tasks.len());
        let mut timeout_policy = Vec::with_capacity(tasks.len());

        for task in tasks {
            id.push(Uuid::new_v4());
            status.push(TaskStatus::Pending.as_str());
            schedule_id.push(Option::<Uuid>::None);
            target.push(OffsetDateTime::from(task.target));
            worker_selector.push(Json(task.worker_selector));

            if task.data_format == TaskDataFormat::Json {
                data_bytes.push(None);
                data_json.push(Some(Json(RawValue::from_string(String::from_utf8(
                    task.data,
                )?)?)));
            } else {
                data_bytes.push(Some(task.data));
                data_json.push(None);
            }

            data_format.push(task.data_format.as_str());
            labels.push(Json(task.labels));
            timeout_policy.push(Json(task.timeout));
        }

        query(
            r#"--sql
            INSERT INTO "ora"."task" (
                "id",
                "status",
                "schedule_id",
                "target",
                "worker_selector",
                "data_bytes",
                "data_json",
                "data_format",
                "labels",
                "timeout_policy"
            ) SELECT 
                "id",
                "status",
                "schedule_id",
                "target",
                "worker_selector",
                "data_bytes",
                "data_json",
                "data_format",
                "labels",
                "timeout_policy"
            FROM UNNEST(
                $1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8,
                $9,
                $10
            ) AS t(
                "id",
                "status",
                "schedule_id",
                "target",
                "worker_selector",
                "data_bytes",
                "data_json",
                "data_format",
                "labels",
                "timeout_policy"
            );
            "#,
        )
        .bind(&id)
        .bind(&status)
        .bind(&schedule_id)
        .bind(&target)
        .bind(&worker_selector)
        .bind(&data_bytes)
        .bind(&data_json)
        .bind(&data_format)
        .bind(&labels)
        .bind(&timeout_policy)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(id)
    }

    async fn tasks_by_ids(
        &self,
        task_ids: Vec<Uuid>,
    ) -> eyre::Result<Vec<Arc<dyn TaskOperations>>> {
        // This is typically called after a batch insertion,
        // so we omit task checks here.
        Ok(task_ids
            .into_iter()
            .map(|task_id| {
                Arc::new(PgTaskOperations {
                    db: self.db.clone(),
                    events: self.events.clone(),
                    task_id,
                }) as Arc<_>
            })
            .collect())
    }
}

#[derive(Debug)]
struct PgTaskOperations {
    task_id: Uuid,
    events: broadcast::Sender<DbEvent>,
    db: PgPool,
}

#[async_trait]
impl TaskOperations for PgTaskOperations {
    fn id(&self) -> Uuid {
        self.task_id
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn status(&self) -> eyre::Result<TaskStatus> {
        Ok(query(
            r#"--sql
            SELECT "status" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<&str, _>(0)?
        .parse()?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn target(&self) -> eyre::Result<ora_common::UnixNanos> {
        Ok(query(
            r#"--sql
            SELECT "target" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<OffsetDateTime, _>(0)?
        .into())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn definition(&self) -> eyre::Result<TaskDefinition> {
        let row = query(
            r#"--sql
            SELECT
                "target",
                "worker_selector",
                "data_format",
                "data_bytes",
                "data_json",
                "timeout_policy",
                "labels"
            FROM
                "ora"."task"
            WHERE
                "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?;
        task_definition_from_row(row).map_err(Into::into)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn result(&self) -> eyre::Result<Option<RawTaskResult>> {
        let row = query(
            r#"--sql
            SELECT
                "status",
                "output_bytes",
                "output_json",
                "output_format",
                "failure_reason"
            FROM
                "ora"."task"
            WHERE
                "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?;

        let status: TaskStatus = row.try_get::<&str, _>("status")?.parse()?;

        match status {
            TaskStatus::Succeeded => {
                let output_format: TaskDataFormat =
                    row.try_get::<&str, _>("output_format")?.parse()?;

                let output = match output_format {
                    TaskDataFormat::Unknown | TaskDataFormat::MessagePack => {
                        row.try_get::<Vec<u8>, _>("output_bytes")?
                    }
                    TaskDataFormat::Json => row
                        .try_get::<Json<&RawValue>, _>("output_json")?
                        .get()
                        .as_bytes()
                        .to_vec(),
                };

                Ok(Some(RawTaskResult::Success {
                    output_format,
                    output,
                }))
            }
            TaskStatus::Failed => Ok(Some(RawTaskResult::Failure {
                reason: row.try_get("failure_reason")?,
            })),
            TaskStatus::Cancelled => Ok(Some(RawTaskResult::Cancelled)),
            _ => Ok(None),
        }
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn wait_result(&self) -> eyre::Result<RawTaskResult> {
        let mut events = self.events.subscribe();
        let mut fallback_interval = tokio::time::interval(std::time::Duration::from_secs(60 * 5));
        loop {
            tokio::select! {
                Ok(ev) = events.recv() => {
                    if let DbEvent::TaskDone(task_id) = ev {
                        if task_id != self.task_id {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                _ = fallback_interval.tick() => {
                    // Fallback so that we don't block forever in case there are no events
                    // for some reason.
                }
            }

            if let Some(output) = self.result().await? {
                return Ok(output);
            }
        }
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn schedule(&self) -> eyre::Result<Option<Arc<dyn ora_client::ScheduleOperations>>> {
        Ok(query(
            r#"--sql
            SELECT "schedule_id" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<Option<Uuid>, _>(0)?
        .map(|schedule_id| {
            Arc::new(PgScheduleOperations {
                schedule_id,
                events: self.events.clone(),
                db: self.db.clone(),
            }) as Arc<_>
        }))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn added_at(&self) -> eyre::Result<ora_common::UnixNanos> {
        Ok(query(
            r#"--sql
            SELECT "added_at" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<OffsetDateTime, _>(0)?
        .into())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn ready_at(&self) -> eyre::Result<Option<ora_common::UnixNanos>> {
        Ok(query(
            r#"--sql
            SELECT "ready_at" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<Option<OffsetDateTime>, _>(0)?
        .map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn started_at(&self) -> eyre::Result<Option<ora_common::UnixNanos>> {
        Ok(query(
            r#"--sql
            SELECT "started_at" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<Option<OffsetDateTime>, _>(0)?
        .map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn succeeded_at(&self) -> eyre::Result<Option<ora_common::UnixNanos>> {
        Ok(query(
            r#"--sql
            SELECT "succeeded_at" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<Option<OffsetDateTime>, _>(0)?
        .map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn failed_at(&self) -> eyre::Result<Option<ora_common::UnixNanos>> {
        Ok(query(
            r#"--sql
            SELECT "failed_at" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<Option<OffsetDateTime>, _>(0)?
        .map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn cancelled_at(&self) -> eyre::Result<Option<ora_common::UnixNanos>> {
        Ok(query(
            r#"--sql
            SELECT "cancelled_at" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<Option<OffsetDateTime>, _>(0)?
        .map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn cancel(&self) -> eyre::Result<()> {
        query(
            r#"--sql
            UPDATE "ora"."task"
            SET
                "status" = 'cancelled',
                "cancelled_at" = NOW()
            WHERE "id" = $1 AND "active"
            RETURNING pg_notify('ora_task_cancelled', "id"::TEXT) AS "notified";
            "#,
        )
        .bind(self.task_id)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn worker_id(&self) -> eyre::Result<Option<Uuid>> {
        let row: (Option<Uuid>,) = query_as(
            r#"--sql
            SELECT
                "worker_id"
            FROM
                "ora"."task"
            WHERE
                "id" = $1
            "#,
        )
        .bind(self.task_id)
        .fetch_one(&self.db)
        .await?;

        Ok(row.0)
    }
}

#[derive(Debug)]
struct PgScheduleOperations {
    schedule_id: Uuid,
    db: PgPool,
    events: broadcast::Sender<DbEvent>,
}

#[async_trait]
impl ScheduleOperations for PgScheduleOperations {
    fn id(&self) -> Uuid {
        self.schedule_id
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn definition(&self) -> eyre::Result<ScheduleDefinition> {
        let row = query(
            r#"--sql
            SELECT
                "schedule_policy",
                "immediate",
                "missed_tasks_policy",
                "new_task",
                "labels"
            FROM
                "ora"."schedule"
            WHERE
                "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.schedule_id)
        .fetch_one(&self.db)
        .await?;

        Ok(ScheduleDefinition {
            policy: row.try_get::<Json<_>, _>("schedule_policy")?.0,
            immediate: row.try_get("immediate")?,
            missed_tasks: row.try_get::<Json<_>, _>("missed_tasks_policy")?.0,
            new_task: row.try_get::<Json<_>, _>("new_task")?.0,
            labels: row.try_get::<Json<_>, _>("labels")?.0,
        })
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn is_active(&self) -> eyre::Result<bool> {
        Ok(query(
            r#"--sql
            SELECT "active" FROM "ora"."schedule"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.schedule_id)
        .fetch_one(&self.db)
        .await?
        .try_get(0)?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn added_at(&self) -> eyre::Result<ora_common::UnixNanos> {
        Ok(query(
            r#"--sql
            SELECT "added_at" FROM "ora"."schedule"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.schedule_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<OffsetDateTime, _>(0)?
        .into())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn cancelled_at(&self) -> eyre::Result<Option<ora_common::UnixNanos>> {
        Ok(query(
            r#"--sql
            SELECT "cancelled_at" FROM "ora"."schedule"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(self.schedule_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<Option<OffsetDateTime>, _>(0)?
        .map(Into::into))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn active_task(&self) -> eyre::Result<Option<Arc<dyn TaskOperations>>> {
        Ok(query(
            r#"--sql
            SELECT "id" FROM "ora"."task"
            WHERE "schedule_id" = $1 AND "active"
            LIMIT 1
            "#,
        )
        .bind(self.schedule_id)
        .try_map(|row| {
            Ok(Arc::new(PgTaskOperations {
                task_id: row.try_get(0)?,
                events: self.events.clone(),
                db: self.db.clone(),
            }) as Arc<_>)
        })
        .fetch_optional(&self.db)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn cancel(&self) -> eyre::Result<()> {
        // We execute the two queries in separate transactions
        // to account for the case where the schedule is cancelled
        // while a task is being added.
        let mut conn = self.db.acquire().await?;
        let mut tx = conn.begin().await?;

        query(
            r#"--sql
            WITH cancel_schedule AS (
                UPDATE "ora"."schedule"
                SET
                    "cancelled_at" = NOW()
                WHERE "id" = $1 AND "active"
                RETURNING pg_notify('ora_schedule_cancelled', "id"::TEXT) AS "notified"
            )
            SELECT 1
            "#,
        )
        .bind(self.schedule_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let mut tx = conn.begin().await?;

        query(
            r#"--sql
            WITH cancel_tasks AS (
                UPDATE "ora"."task"
                SET
                    "status" = 'cancelled',
                    "cancelled_at" = NOW()
                WHERE "schedule_id" = $1 AND "active"
                RETURNING pg_notify('ora_task_cancelled', "id"::TEXT) AS "notified"
            )
            SELECT 1
            "#,
        )
        .bind(self.schedule_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }
}

#[async_trait]
impl SchedulerTaskStore for DbStore<Postgres> {
    type Error = sqlx::Error;
    type Events = BoxStream<'static, Result<SchedulerTaskStoreEvent, Self::Error>>;

    #[tracing::instrument(level = "debug", skip_all)]
    async fn events(&self) -> Result<Self::Events, Self::Error> {
        let mut stream = self.events.subscribe();

        Ok(Box::pin(try_stream!({
            loop {
                match stream.recv().await {
                    Ok(event) => {
                        if let DbEvent::SchedulerTaskStore(event) = event {
                            yield event;
                        }
                    }
                    Err(error) => match error {
                        RecvError::Closed => {
                            return;
                        }
                        RecvError::Lagged(n) => {
                            panic!("in-memory channel lagged by {n}, increase the channel size to support larger volumes of events");
                        }
                    },
                }
            }
        })))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn pending_tasks(&self) -> Result<Vec<PendingTask>, Self::Error> {
        Ok(query(
            r#"--sql
            SELECT
                "id",
                "target",
                "timeout_policy"
            FROM "ora"."task"
            WHERE
                "status" = 'pending'
            "#,
        )
        .try_map(|row: PgRow| {
            Ok(PendingTask {
                id: row.try_get("id")?,
                target: row.try_get::<OffsetDateTime, _>("target")?.into(),
                timeout: row.try_get::<Json<_>, _>("timeout_policy")?.0,
            })
        })
        .fetch_all(&self.db)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn active_tasks(&self) -> Result<Vec<ActiveTask>, Self::Error> {
        Ok(query(
            r#"--sql
            SELECT
                "id",
                "target",
                "timeout_policy"
            FROM "ora"."task"
            WHERE
                "status" = 'ready'
                OR "status" = 'running'
            "#,
        )
        .try_map(|row: PgRow| {
            Ok(ActiveTask {
                id: row.try_get("id")?,
                target: row.try_get::<OffsetDateTime, _>("target")?.into(),
                timeout: row.try_get::<Json<_>, _>("timeout_policy")?.0,
            })
        })
        .fetch_all(&self.db)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_ready(&self, task_id: Uuid) -> Result<(), Self::Error> {
        query(
            r#"--sql
            UPDATE "ora"."task"
            SET
                "status" = 'ready',
                "ready_at" = NOW()
            WHERE "id" = $1 AND "active"
            RETURNING pg_notify('ora_task_ready', "id"::TEXT) AS "notified";
            "#,
        )
        .bind(task_id)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_timed_out(&self, task_id: Uuid) -> Result<(), Self::Error> {
        query(
            r#"--sql
            UPDATE "ora"."task"
            SET
                "status" = 'failed',
                "failure_reason" = 'task timeout reached',
                "failed_at" = NOW()
            WHERE "id" = $1 AND "active"
            RETURNING pg_notify('ora_task_failed', "id"::TEXT) AS "notified";
            "#,
        )
        .bind(task_id)
        .execute(&self.db)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl SchedulerScheduleStore for DbStore<Postgres> {
    type Error = sqlx::Error;
    type Events = BoxStream<'static, Result<SchedulerScheduleStoreEvent, Self::Error>>;

    #[tracing::instrument(level = "debug", skip_all)]
    async fn events(&self) -> Result<Self::Events, Self::Error> {
        let mut stream = self.events.subscribe();
        Ok(Box::pin(try_stream!({
            loop {
                match stream.recv().await {
                    Ok(event) => {
                        if let DbEvent::SchedulerScheduleStore(event) = event {
                            yield event;
                        }
                    }
                    Err(error) => match error {
                        RecvError::Closed => {
                            return;
                        }
                        RecvError::Lagged(n) => {
                            panic!("in-memory channel lagged by {n}, increase the channel size to support larger volumes of events");
                        }
                    },
                }
            }
        })))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn pending_schedules(&self) -> Result<Vec<ActiveSchedule>, Self::Error> {
        Ok(query(
            r#"--sql
                SELECT
                    "id",
                    "schedule_policy",
                    "immediate",
                    "missed_tasks_policy",
                    "new_task",
                    "labels"
                FROM
                    "ora"."schedule"
                WHERE
                    "active" AND NOT EXISTS (
                        SELECT 1 FROM "ora"."task"
                        WHERE
                            "active" AND "schedule_id" = "ora"."schedule"."id"
                        LIMIT 1
                    )
                "#,
        )
        .try_map(|row: PgRow| {
            Ok(ActiveSchedule {
                id: row.try_get("id")?,
                definition: ScheduleDefinition {
                    policy: row.try_get::<Json<_>, _>("schedule_policy")?.0,
                    immediate: row.try_get("immediate")?,
                    missed_tasks: row.try_get::<Json<_>, _>("missed_tasks_policy")?.0,
                    new_task: row.try_get::<Json<_>, _>("new_task")?.0,
                    labels: row.try_get::<Json<_>, _>("labels")?.0,
                },
            })
        })
        .fetch_all(&self.db)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn pending_schedule_of_task(
        &self,
        task_id: Uuid,
    ) -> Result<Option<ActiveSchedule>, Self::Error> {
        Ok(query(
            r#"--sql
                SELECT
                    "ora"."schedule"."id",
                    "ora"."schedule"."schedule_policy",
                    "ora"."schedule"."immediate",
                    "ora"."schedule"."missed_tasks_policy",
                    "ora"."schedule"."new_task",
                    "ora"."schedule"."labels"
                FROM
                    "ora"."task"
                JOIN
                    "ora"."schedule"
                ON
                    "ora"."task"."schedule_id" = "ora"."schedule"."id"
                WHERE
                    "ora"."task"."id" = $1
                    AND "ora"."schedule"."active"
                    AND NOT EXISTS (
                        SELECT
                            1
                        FROM
                            "ora"."task"
                        WHERE
                            "schedule_id" = "ora"."schedule"."id" AND "active"
                        LIMIT 1
                    )
                LIMIT 1
                "#,
        )
        .bind(task_id)
        .try_map(|row: PgRow| {
            Ok(ActiveSchedule {
                id: row.try_get("id")?,
                definition: ScheduleDefinition {
                    policy: row.try_get::<Json<_>, _>("schedule_policy")?.0,
                    immediate: row.try_get("immediate")?,
                    missed_tasks: row.try_get::<Json<_>, _>("missed_tasks_policy")?.0,
                    new_task: row.try_get::<Json<_>, _>("new_task")?.0,
                    labels: row.try_get::<Json<_>, _>("labels")?.0,
                },
            })
        })
        .fetch_optional(&self.db)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_target(&self, task_id: Uuid) -> Result<ora_common::UnixNanos, Self::Error> {
        Ok(query(
            r#"--sql
            SELECT "target" FROM "ora"."task"
            WHERE "id" = $1
            LIMIT 1
            "#,
        )
        .bind(task_id)
        .fetch_one(&self.db)
        .await?
        .try_get::<OffsetDateTime, _>(0)?
        .into())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn add_task(&self, schedule_id: Uuid, task: TaskDefinition) -> Result<(), Self::Error> {
        self.add_task_with(task, Some(schedule_id)).await?;
        Ok(())
    }
}

#[async_trait]
impl WorkerStore for DbStore<Postgres> {
    type Error = sqlx::Error;
    type Events = BoxStream<'static, Result<WorkerStoreEvent, Self::Error>>;

    #[tracing::instrument(level = "debug", skip_all)]
    async fn events(&self, selectors: &[WorkerSelector]) -> Result<Self::Events, Self::Error> {
        let mut stream = self.events.subscribe();

        self.update_worker_selectors(selectors);

        let selectors: HashSet<WorkerSelector> = selectors.iter().cloned().collect();
        Ok(Box::pin(try_stream!({
            loop {
                match stream.recv().await {
                    Ok(event) => {
                        if let DbEvent::WorkerStore(event) = event {
                            if let WorkerStoreEvent::TaskReady(task) = &event {
                                if selectors.contains(&task.definition.worker_selector) {
                                    yield event;
                                }
                            }
                        }
                    }
                    Err(error) => match error {
                        RecvError::Closed => {
                            return;
                        }
                        RecvError::Lagged(n) => {
                            panic!("in-memory channel lagged by {n}, increase the channel size to support larger volumes of events");
                        }
                    },
                }
            }
        })))
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn ready_tasks(
        &self,
        selectors: &[WorkerSelector],
    ) -> Result<Vec<ReadyTask>, Self::Error> {
        self.update_worker_selectors(selectors);
        Ok(query(
            r#"--sql
            SELECT
                "id",
                "target",
                "worker_selector",
                "data_format",
                "data_bytes",
                "data_json",
                "timeout_policy",
                "labels"
            FROM
                "ora"."task"
            JOIN UNNEST($1::JSONB[]) AS ws("expected_selector") ON
                "worker_selector" @> "expected_selector"
                    AND "worker_selector" <@ "expected_selector"
            WHERE
                "status" = 'ready'
            "#,
        )
        .bind(selectors.iter().map(Json).collect::<Vec<_>>())
        .try_map(|row: PgRow| {
            Ok(ReadyTask {
                id: row.try_get("id")?,
                definition: task_definition_from_row(row)?,
            })
        })
        .fetch_all(&self.db)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn select_task(&self, task_id: Uuid, worker_id: Uuid) -> Result<bool, Self::Error> {
        let mut tx = self.db.begin().await?;

        query(
            r#"--sql
            SELECT
                *
            FROM
                "ora"."task"
            WHERE
                "id" = $1
            FOR UPDATE
            "#,
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await?;

        let res = query(
            r#"--sql
            UPDATE
                "ora"."task"
            SET
                "worker_id" = $2
            WHERE
                "id" = $1
                AND "worker_id" IS NULL
            "#,
        )
        .bind(task_id)
        .bind(worker_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let should_select = res.rows_affected() > 0;

        Ok(should_select)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_started(&self, task_id: Uuid) -> Result<(), Self::Error> {
        query(
            r#"--sql
            UPDATE "ora"."task"
            SET
                "status" = 'started',
                "started_at" = NOW()
            WHERE "id" = $1 AND "active"
            RETURNING pg_notify('ora_task_started', "id"::TEXT) AS "notified";
            "#,
        )
        .bind(task_id)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_succeeded(
        &self,
        task_id: Uuid,
        output: Vec<u8>,
        output_format: TaskDataFormat,
    ) -> Result<(), Self::Error> {
        match output_format {
            TaskDataFormat::Json => {
                query(
                    r#"--sql
                    UPDATE "ora"."task"
                    SET
                        "status" = 'succeeded',
                        "output_json" = $2,
                        "output_format" = $3,
                        "succeeded_at" = NOW()
                    WHERE "id" = $1 AND "active"
                    RETURNING pg_notify('ora_task_succeeded', "id"::TEXT) AS "notified";
                    "#,
                )
                .bind(task_id)
                .bind(
                    serde_json::from_slice::<Value>(&output)
                        .tap_err(|error| {
                            tracing::error!(error = ?error, "failed to parse task output JSON");
                        })
                        .unwrap_or_default(),
                )
                .bind(output_format.as_str())
                .execute(&self.db)
                .await?;
            }
            _ => {
                query(
                    r#"--sql
                    UPDATE "ora"."task"
                    SET
                        "status" = 'succeeded',
                        "output_bytes" = $2,
                        "output_format" = $3,
                        "succeeded_at" = NOW()
                    WHERE "id" = $1 AND "active";
                    "#,
                )
                .bind(task_id)
                .bind(output)
                .bind(output_format.as_str())
                .execute(&self.db)
                .await?;
            }
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_failed(&self, task_id: Uuid, reason: String) -> Result<(), Self::Error> {
        query(
            r#"--sql
            UPDATE "ora"."task"
            SET
                "status" = 'failed',
                "failure_reason" = $2,
                "failed_at" = NOW()
            WHERE "id" = $1 AND "active"
            RETURNING pg_notify('ora_task_failed', "id"::TEXT) AS "notified";
            "#,
        )
        .bind(task_id)
        .bind(reason)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn task_cancelled(&self, task_id: Uuid) -> Result<(), Self::Error> {
        query(
            r#"--sql
            UPDATE "ora"."task"
            SET
                "status" = 'cancelled',
                "cancelled_at" = NOW()
            WHERE "id" = $1 AND "active"
            RETURNING pg_notify('ora_task_cancelled', "id"::TEXT) AS "notified";
            "#,
        )
        .bind(task_id)
        .execute(&self.db)
        .await?;

        Ok(())
    }
}

#[tracing::instrument(level = "debug", skip_all)]
fn filter_tasks(options: &Tasks) -> impl IntoCondition {
    Condition::all()
        .add_option(
            options
                .include_status
                .as_ref()
                .map(|status| Expr::col(Task::Status).is_in(status.iter().map(TaskStatus::as_str))),
        )
        .add_option(
            options
                .schedule_id
                .map(|schedule_id| Expr::col(Task::ScheduleId).eq(schedule_id)),
        )
        .add_option(options.kind.as_ref().map(|kind| {
            Expr::cust_with_values(
                r#""worker_selector" @> build_jsonb_object('kind', $1)"#,
                [kind],
            )
        }))
        .add_option(options.search.as_ref().map(|search| {
            let like = format!("%{search}%");

            Expr::col(Task::DataJson)
                .cast_as(Alias::new("TEXT"))
                .like(&like)
                .or(Expr::col(Task::OutputJson)
                    .cast_as(Alias::new("TEXT"))
                    .like(&like))
                .or(Expr::col(Task::WorkerSelector)
                    .cast_as(Alias::new("TEXT"))
                    .like(&like))
                .or(Expr::col(Task::Labels)
                    .cast_as(Alias::new("TEXT"))
                    .like(&like))
        }))
        .add_option(options.include_labels.as_ref().map(|labels| {
            let mut label_cond = Condition::all();

            for label in labels {
                match &label.value {
                    ora_client::LabelValueMatch::AnyValue => {
                        label_cond = label_cond
                            .add(Expr::cust_with_values(r#""labels" ? $1"#, [&label.label]));
                    }
                    ora_client::LabelValueMatch::Value(label_value) => {
                        label_cond =
                            label_cond.add(Expr::cust_with_values::<_, sea_query::Value, _>(
                                r#""labels" @> jsonb_build_object($1, $2)"#,
                                [(label.label.as_str()).into(), label_value.clone().into()],
                            ));
                    }
                }
            }

            label_cond
        }))
        .add_option(
            options
                .added_after
                .map(|added_after| Expr::col(Task::AddedAt).gte(added_after)),
        )
        .add_option(
            options
                .added_before
                .map(|added_before| Expr::col(Task::AddedAt).lt(added_before)),
        )
        .add_option(options.finished_after.map(|finished_after| {
            Condition::all().add(Expr::col(Task::Active).not()).add(
                Condition::any()
                    .add(Expr::col(Task::SucceededAt).gte(finished_after))
                    .add(Expr::col(Task::FailedAt).gte(finished_after))
                    .add(Expr::col(Task::CancelledAt).gte(finished_after)),
            )
        }))
        .add_option(options.finished_before.map(|finished_before| {
            Condition::all().add(Expr::col(Task::Active).not()).add(
                Condition::any()
                    .add(Expr::col(Task::SucceededAt).lt(finished_before))
                    .add(Expr::col(Task::FailedAt).lt(finished_before))
                    .add(Expr::col(Task::CancelledAt).lt(finished_before)),
            )
        }))
        .add_option(
            options
                .target_after
                .map(|target_after| Expr::col(Task::Target).gte(target_after)),
        )
        .add_option(
            options
                .target_before
                .map(|target_before| Expr::col(Task::Target).lt(target_before)),
        )
}

#[tracing::instrument(level = "debug", skip_all)]
fn filter_schedules(options: &Schedules) -> impl IntoCondition {
    Condition::all()
        .add_option(
            options
                .active
                .map(|active| Expr::col(Schedule::Active).eq(active)),
        )
        .add_option(options.kind.as_ref().map(|kind| {
            Expr::cust_with_values(r#"(jsonb_path_query_first("new_task", '$.*.*.worker_selector.kind') #>> '{}') = $1"#, [kind])
        }))
        .add_option(options.search.as_ref().map(|search| {
            let like = format!("%{search}%");

            Expr::col(Schedule::NewTask)
                .cast_as(Alias::new("TEXT"))
                .like(&like)
                .or(Expr::col(Schedule::Labels)
                    .cast_as(Alias::new("TEXT"))
                    .like(&like))
        }))
        .add_option(options.include_labels.as_ref().map(|labels| {
            let mut label_cond = Condition::all();

            for label in labels {
                match &label.value {
                    ora_client::LabelValueMatch::AnyValue => {
                        label_cond = label_cond
                            .add(Expr::cust_with_values(r#""labels" ? $1"#, [&label.label]));
                    }
                    ora_client::LabelValueMatch::Value(label_value) => {
                        label_cond =
                            label_cond.add(Expr::cust_with_values::<_, sea_query::Value, _>(
                                r#""labels" @> jsonb_build_object($1, $2)"#,
                                [(label.label.as_str()).into(), label_value.clone().into()],
                            ));
                    }
                }
            }

            label_cond
        }))
        .add_option(
            options
                .added_after
                .map(|added_after| Expr::col(Schedule::AddedAt).gte(added_after)),
        )
        .add_option(
            options
                .added_before
                .map(|added_before| Expr::col(Schedule::AddedAt).lt(added_before)),
        )
        .add_option(
            options
                .cancelled_after
                .map(|cancelled_after| Expr::col(Schedule::CancelledAt).gte(cancelled_after)),
        )
        .add_option(
            options
                .cancelled_before
                .map(|cancelled_before| Expr::col(Schedule::CancelledAt).lt(cancelled_before)),
        )
}
