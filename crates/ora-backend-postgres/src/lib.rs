//! Postgres backend implementation for Ora.
#![allow(missing_docs)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use deadpool_postgres::{Pool, PoolError};
use futures::Stream;
use ora_backend::{
    Backend,
    common::{NextPageToken, TimeRange},
    executions::{
        ExecutionId, FailedExecution, InProgressExecution, ReadyExecution, StartedExecution,
        SucceededExecution,
    },
    executors::ExecutorId,
    jobs::{
        AddedJobs, CancelledJob, JobDetails, JobFilters, JobId, JobOrderBy, JobType, JobTypeId,
        NewJob,
    },
    schedules::{
        AddedSchedules, PendingSchedule, ScheduleDefinition, ScheduleDetails, ScheduleFilters,
        ScheduleId, ScheduleOrderBy, StoppedSchedule,
    },
};

use refinery::embed_migrations;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    db::{DbPool, DbTransaction},
    query::{
        jobs::{cancel_jobs, job_count},
        schedules::schedule_count,
    },
};

embed_migrations!("./migrations");

mod db;
mod models;
mod query;

/// Postgres backend for Ora.
#[must_use]
pub struct PostgresBackend {
    pool: DbPool,
    delete_batch_size: usize,
}

type Result<T> = core::result::Result<T, Error>;

/// Errors that can occur when using the Postgres backend.
#[derive(Error, Debug)]
pub enum Error {
    /// An error occurred during database migrations.
    #[error("{0}")]
    Migrations(#[from] refinery::Error),
    /// An error occurred while acquiring a database connection from the pool.
    #[error("{0}")]
    Pool(#[from] PoolError),
    /// An error occurred during a database operation.
    #[error("{0}")]
    Postgres(#[from] tokio_postgres::Error),
    /// An error occurred while serializing or deserializing JSON data.
    #[error("{0}")]
    Serde(#[from] serde_json::Error),
    /// An error occurred while converting a value to or from a database type.
    #[error("invalid page token: {0}")]
    InvalidPageToken(Box<dyn std::error::Error + Send + Sync>),
}

impl PostgresBackend {
    /// Create and initialize a new Postgres backend
    /// with the given connection pool.
    pub async fn new(pool: Pool) -> Result<Self> {
        let mut conn = pool.get().await?;
        conn.execute("CREATE SCHEMA IF NOT EXISTS ora", &[]).await?;
        migrations::runner()
            .set_migration_table_name("ora.migrations")
            .run_async(&mut **conn)
            .await?;
        Ok(Self {
            pool: DbPool(pool),
            delete_batch_size: 40_000,
        })
    }

    /// Set the batch size for all delete operations.
    ///
    /// # Panics
    ///
    /// Panics if `batch_size` is zero.
    pub fn with_delete_batch_size(mut self, batch_size: usize) -> Self {
        assert!(batch_size > 0, "batch size must be greater than zero");
        self.delete_batch_size = batch_size;
        self
    }
}

impl Backend for PostgresBackend {
    type Error = Error;

    async fn add_job_types(&self, job_types: &[JobType]) -> Result<()> {
        if job_types.is_empty() {
            return Ok(());
        }

        let mut conn = self.pool.get().await?;

        let tx = conn.transaction().await?;

        let mut col_id = Vec::with_capacity(job_types.len());
        let mut col_description = Vec::with_capacity(job_types.len());
        let mut col_input_schema_json = Vec::with_capacity(job_types.len());
        let mut col_output_schema_json = Vec::with_capacity(job_types.len());

        for job_type in job_types {
            col_id.push(job_type.id.as_str());
            col_description.push(job_type.description.as_deref());
            col_input_schema_json.push(job_type.input_schema_json.as_deref());
            col_output_schema_json.push(job_type.output_schema_json.as_deref());
        }

        {
            let stmt = tx
                .prepare(
                    r#"--sql
                    INSERT INTO ora.job_type (
                        id,
                        description,
                        input_schema_json,
                        output_schema_json
                    ) SELECT * FROM UNNEST(
                        $1::TEXT[],
                        $2::TEXT[],
                        $3::TEXT[],
                        $4::TEXT[]
                    ) ON CONFLICT (id) DO UPDATE SET
                        description = EXCLUDED.description,
                        input_schema_json = EXCLUDED.input_schema_json,
                        output_schema_json = EXCLUDED.output_schema_json
                    WHERE
                        ora.job_type.description IS DISTINCT FROM EXCLUDED.description
                        OR ora.job_type.input_schema_json IS DISTINCT FROM EXCLUDED.input_schema_json
                        OR ora.job_type.output_schema_json IS DISTINCT FROM EXCLUDED.output_schema_json
                    "#,
                )
                .await?;

            tx.execute(
                &stmt,
                &[
                    &col_id,
                    &col_description,
                    &col_input_schema_json,
                    &col_output_schema_json,
                ],
            )
            .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    async fn list_job_types(&self) -> Result<Vec<JobType>> {
        let mut conn = self.pool.get().await?;

        let tx = conn.read_only_transaction().await?;

        let job_types = {
            let stmt = tx
                .prepare(
                    r#"--sql
                    SELECT
                        id,
                        description,
                        input_schema_json,
                        output_schema_json
                    FROM
                        ora.job_type
                    ORDER BY id
                    "#,
                )
                .await?;

            let rows = tx.query(&stmt, &[]).await?;

            rows.into_iter()
                .map(|row| {
                    Result::<_>::Ok(JobType {
                        id: JobTypeId::new_unchecked(row.try_get::<_, String>(0)?),
                        description: row.try_get(1)?,
                        input_schema_json: row.try_get(2)?,
                        output_schema_json: row.try_get(3)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?
        };

        tx.commit().await?;

        Ok(job_types)
    }

    async fn add_jobs(
        &self,
        jobs: &[NewJob],
        if_not_exists: Option<JobFilters>,
    ) -> Result<AddedJobs> {
        if jobs.is_empty() {
            return Ok(AddedJobs::Added(Vec::new()));
        }

        let mut conn = self.pool.get().await?;

        let tx = conn.transaction().await?;

        if let Some(filters) = if_not_exists {
            let existing_job_ids = query::jobs::job_ids(&tx, filters).await?;

            if !existing_job_ids.is_empty() {
                tx.commit().await?;
                return Ok(AddedJobs::Existing(existing_job_ids));
            }
        }

        let mut col_id = Vec::with_capacity(jobs.len());
        let mut col_schedule_id = Vec::with_capacity(jobs.len());

        {
            let mut col_job_type_id = Vec::with_capacity(jobs.len());
            let mut col_target_execution_time = Vec::with_capacity(jobs.len());
            let mut col_input_payload_json = Vec::with_capacity(jobs.len());
            let mut col_timeout_policy_json = Vec::with_capacity(jobs.len());
            let mut col_retry_policy_json = Vec::with_capacity(jobs.len());

            for job in jobs {
                col_id.push(Uuid::now_v7());
                col_job_type_id.push(job.job.job_type_id.as_str());
                col_target_execution_time.push(job.job.target_execution_time);
                col_input_payload_json.push(job.job.input_payload_json.as_str());
                col_timeout_policy_json.push(serde_json::to_string(&job.job.timeout_policy)?);
                col_retry_policy_json.push(serde_json::to_string(&job.job.retry_policy)?);
                col_schedule_id.push(job.schedule_id.map(|s| s.0));
            }

            let stmt = tx
                .prepare(
                    r#"--sql
                    INSERT INTO ora.job (
                        id,
                        job_type_id,
                        target_execution_time,
                        input_payload_json,
                        timeout_policy_json,
                        retry_policy_json,
                        schedule_id
                    ) SELECT * FROM UNNEST(
                        $1::UUID[],
                        $2::TEXT[],
                        $3::TIMESTAMPTZ[],
                        $4::TEXT[],
                        $5::TEXT[],
                        $6::TEXT[],
                        $7::UUID[]
                    )
                    "#,
                )
                .await?;

            tx.execute(
                &stmt,
                &[
                    &col_id,
                    &col_job_type_id,
                    &col_target_execution_time,
                    &col_input_payload_json,
                    &col_timeout_policy_json,
                    &col_retry_policy_json,
                    &col_schedule_id,
                ],
            )
            .await?;
        }

        {
            let mut col_job_id = Vec::with_capacity(jobs.len());
            let mut col_job_label_key = Vec::with_capacity(jobs.len());
            let mut col_job_label_value = Vec::with_capacity(jobs.len());

            for (i, job) in jobs.iter().enumerate() {
                for label in &job.job.labels {
                    col_job_id.push(col_id[i]);
                    col_job_label_key.push(label.key.as_str());
                    col_job_label_value.push(label.value.as_str());
                }
            }

            let stmt = tx
                .prepare(
                    r#"--sql
                    INSERT INTO ora.job_label (
                        job_id,
                        label_key,
                        label_value
                    ) SELECT * FROM UNNEST(
                        $1::UUID[],
                        $2::TEXT[],
                        $3::TEXT[]
                    )
                    "#,
                )
                .await?;

            tx.execute(
                &stmt,
                &[&col_job_id, &col_job_label_key, &col_job_label_value],
            )
            .await?;
        }

        {
            let stmt = tx
                .prepare(
                    r#"--sql
                    UPDATE ora.schedule
                    SET
                        active_job_id = t.job_id
                    FROM UNNEST(
                        $1::UUID[],
                        $2::UUID[]
                    ) AS t(job_id, schedule_id)
                    WHERE
                        ora.schedule.id = t.schedule_id
                    "#,
                )
                .await?;

            tx.execute(&stmt, &[&col_id, &col_schedule_id]).await?;
        }

        let job_ids = col_id.into_iter().map(Into::into).collect::<Vec<_>>();
        add_executions(&tx, &job_ids).await?;

        tx.commit().await?;

        Ok(AddedJobs::Added(job_ids))
    }

    async fn list_jobs(
        &self,
        filters: JobFilters,
        order_by: Option<JobOrderBy>,
        page_size: u32,
        page_token: Option<NextPageToken>,
    ) -> Result<(Vec<JobDetails>, Option<ora_backend::common::NextPageToken>)> {
        let mut conn = self.pool.get().await?;
        let tx = conn.read_only_transaction().await?;

        let (jobs, next_page_token) = query::jobs::job_details(
            &tx,
            filters,
            order_by.unwrap_or(JobOrderBy::CreatedAtAsc),
            page_size,
            page_token.map(|t| t.0),
        )
        .await?;

        tx.commit().await?;

        Ok((jobs, next_page_token))
    }

    async fn count_jobs(&self, filters: JobFilters) -> Result<u64> {
        let mut conn = self.pool.get().await?;
        let tx = conn.read_only_transaction().await?;
        let count = u64::try_from(job_count(&tx, filters).await?).unwrap_or_default();
        tx.commit().await?;
        Ok(count)
    }

    async fn cancel_jobs(&self, filters: JobFilters) -> Result<Vec<CancelledJob>> {
        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;
        let now = std::time::SystemTime::now();

        let jobs = cancel_jobs(&tx, filters).await?;
        mark_jobs_inactive(
            &tx,
            &jobs.iter().map(|j| (j.job_id, now)).collect::<Vec<_>>(),
        )
        .await?;
        tx.commit().await?;

        Ok(jobs)
    }

    async fn add_schedules(
        &self,
        schedules: &[ScheduleDefinition],
        if_not_exists: Option<ScheduleFilters>,
    ) -> Result<AddedSchedules> {
        if schedules.is_empty() {
            return Ok(AddedSchedules::Added(Vec::new()));
        }

        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;

        if let Some(filters) = if_not_exists {
            let existing_schedule_ids = query::schedules::schedule_ids(&tx, filters).await?;

            if !existing_schedule_ids.is_empty() {
                tx.commit().await?;
                return Ok(AddedSchedules::Existing(existing_schedule_ids));
            }
        }

        let mut col_id = Vec::with_capacity(schedules.len());
        let mut col_job_template_job_type_id = Vec::with_capacity(schedules.len());
        let mut col_job_template_json = Vec::with_capacity(schedules.len());
        let mut col_scheduling_policy_json = Vec::with_capacity(schedules.len());
        let mut col_start_after = Vec::with_capacity(schedules.len());
        let mut col_end_before = Vec::with_capacity(schedules.len());

        for schedule in schedules {
            let (start_after, end_before) = (schedule.time_range.start, schedule.time_range.end);

            col_id.push(Uuid::now_v7());
            col_job_template_job_type_id.push(schedule.job_template.job_type_id.as_str());
            col_job_template_json.push(serde_json::to_string(&schedule.job_template)?);
            col_scheduling_policy_json.push(serde_json::to_string(&schedule.scheduling)?);
            col_start_after.push(start_after);
            col_end_before.push(end_before);
        }

        {
            let stmt = tx
                .prepare(
                    r#"--sql
                    INSERT INTO ora.schedule (
                        id,
                        job_template_job_type_id,
                        job_template_json,
                        scheduling_policy_json,
                        start_after,
                        end_before
                    ) SELECT * FROM UNNEST(
                        $1::UUID[],
                        $2::TEXT[],
                        $3::TEXT[],
                        $4::TEXT[],
                        $5::TIMESTAMPTZ[],
                        $6::TIMESTAMPTZ[]
                    )
                    "#,
                )
                .await?;

            tx.execute(
                &stmt,
                &[
                    &col_id,
                    &col_job_template_job_type_id,
                    &col_job_template_json,
                    &col_scheduling_policy_json,
                    &col_start_after,
                    &col_end_before,
                ],
            )
            .await?;
        }

        {
            let mut col_schedule_id = Vec::with_capacity(schedules.len());
            let mut col_schedule_label_key = Vec::with_capacity(schedules.len());
            let mut col_schedule_label_value = Vec::with_capacity(schedules.len());

            for (i, schedule) in schedules.iter().enumerate() {
                for label in &schedule.labels {
                    col_schedule_id.push(col_id[i]);
                    col_schedule_label_key.push(label.key.as_str());
                    col_schedule_label_value.push(label.value.as_str());
                }
            }

            let stmt = tx
                .prepare(
                    r#"--sql
                    INSERT INTO ora.schedule_label (
                        schedule_id,
                        label_key,
                        label_value
                    ) SELECT * FROM UNNEST(
                        $1::UUID[],
                        $2::TEXT[],
                        $3::TEXT[]
                    )
                    "#,
                )
                .await?;

            tx.execute(
                &stmt,
                &[
                    &col_schedule_id,
                    &col_schedule_label_key,
                    &col_schedule_label_value,
                ],
            )
            .await?;
        }

        tx.commit().await?;

        let schedule_ids = col_id.into_iter().map(ScheduleId).collect::<Vec<_>>();

        Ok(AddedSchedules::Added(schedule_ids))
    }

    async fn list_schedules(
        &self,
        filters: ScheduleFilters,
        order_by: Option<ScheduleOrderBy>,
        page_size: u32,
        page_token: Option<NextPageToken>,
    ) -> Result<(
        Vec<ScheduleDetails>,
        Option<ora_backend::common::NextPageToken>,
    )> {
        let mut conn = self.pool.get().await?;
        let tx = conn.read_only_transaction().await?;

        let (schedules, next_page_token) = query::schedules::schedule_details(
            &tx,
            filters,
            order_by.unwrap_or(ScheduleOrderBy::CreatedAtAsc),
            page_size,
            page_token.map(|t| t.0),
        )
        .await?;

        tx.commit().await?;

        Ok((schedules, next_page_token))
    }

    async fn count_schedules(&self, filters: ScheduleFilters) -> Result<u64> {
        let mut conn = self.pool.get().await?;
        let tx = conn.read_only_transaction().await?;
        let count = u64::try_from(schedule_count(&tx, filters).await?).unwrap_or_default();
        tx.commit().await?;
        Ok(count)
    }

    async fn stop_schedules(&self, filters: ScheduleFilters) -> Result<Vec<StoppedSchedule>> {
        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;
        let schedules = query::schedules::stop_schedules(&tx, filters).await?;
        tx.commit().await?;
        Ok(schedules)
    }

    fn ready_executions(&self) -> impl Stream<Item = Result<Vec<ReadyExecution>>> + Send {
        async_stream::try_stream!({
            let mut last_execution_id: Option<ExecutionId> = None;

            loop {
                let mut conn = self.pool.get().await?;
                let tx = conn.read_only_transaction().await?;

                let rows = if let Some(last_execution_id) = last_execution_id {
                    let stmt = tx
                        .prepare(
                            r#"--sql
                            SELECT
                                ora.execution.id,
                                ora.job.id,
                                ora.job.job_type_id,
                                ora.job.input_payload_json,
                                (SELECT COUNT(*) FROM ora.execution ex WHERE ex.job_id = ora.job.id),
                                ora.job.retry_policy_json,
                                EXTRACT(EPOCH FROM ora.job.target_execution_time)::DOUBLE PRECISION
                            FROM
                                ora.execution
                            JOIN ora.job ON
                                ora.execution.job_id = ora.job.id
                            WHERE
                                status = 0
                                AND ora.job.target_execution_time <= NOW()
                                AND ora.execution.id > $1::UUID
                            ORDER BY
                                ora.execution.id ASC
                            LIMIT 1000
                            "#,
                        )
                        .await?;

                    tx.query(&stmt, &[&last_execution_id.0]).await?
                } else {
                    let stmt = tx
                        .prepare(
                            r#"--sql
                            SELECT
                                ora.execution.id,
                                ora.job.id,
                                ora.job.job_type_id,
                                ora.job.input_payload_json,
                                (SELECT COUNT(*) FROM ora.execution ex WHERE ex.job_id = ora.job.id),
                                ora.job.retry_policy_json,
                                EXTRACT(EPOCH FROM ora.job.target_execution_time)::DOUBLE PRECISION
                            FROM
                                ora.execution
                            JOIN ora.job ON
                                ora.execution.job_id = ora.job.id
                            WHERE
                                status = 0
                                AND ora.job.target_execution_time <= NOW()
                            ORDER BY
                                ora.execution.id ASC
                            LIMIT 1000
                            "#,
                        )
                        .await?;

                    tx.query(&stmt, &[]).await?
                };

                tx.commit().await?;

                if rows.is_empty() {
                    break;
                }

                let mut ready_executions = Vec::with_capacity(rows.len());

                for row in rows {
                    ready_executions.push(ReadyExecution {
                        execution_id: ExecutionId(row.try_get(0)?),
                        job_id: JobId(row.try_get(1)?),
                        job_type_id: JobTypeId::new_unchecked(row.try_get::<_, String>(2)?),
                        input_payload_json: row.try_get(3)?,
                        attempt_number: row.try_get::<_, i64>(4)? as u64,
                        retry_policy: serde_json::from_str(&row.try_get::<_, String>(5)?)?,
                        target_execution_time: UNIX_EPOCH
                            + std::time::Duration::from_secs_f64(row.try_get::<_, f64>(6)?),
                    });
                }

                last_execution_id = ready_executions.last().map(|e| e.execution_id);

                yield ready_executions;
            }
        })
    }

    async fn wait_for_ready_executions(&self, ignore: &[ExecutionId]) -> crate::Result<()> {
        let ignored_executions = ignore.iter().map(|id| id.0).collect::<Vec<_>>();

        loop {
            let mut conn = self.pool.get().await?;

            let next_timestamp = {
                let tx = conn.read_only_transaction().await?;

                let stmt = tx
                    .prepare(
                        r#"--sql
                        SELECT
                            EXTRACT(EPOCH FROM target_execution_time)::DOUBLE PRECISION
                        FROM
                            ora.job
                        JOIN ora.execution ON
                            ora.execution.job_id = ora.job.id
                        WHERE
                            ora.execution.status = 0
                            AND NOT (ora.execution.id = ANY($1::UUID[]))
                        ORDER BY
                            ora.job.target_execution_time ASC
                        LIMIT 1
                        "#,
                    )
                    .await?;

                let row = tx.query_opt(&stmt, &[&ignored_executions]).await?;

                tx.commit().await?;

                match row {
                    Some(row) => row
                        .try_get::<_, Option<f64>>(0)?
                        .map(|ts| UNIX_EPOCH + std::time::Duration::from_secs_f64(ts)),
                    None => None,
                }
            };

            drop(conn);

            if let Some(next_timestamp) = next_timestamp {
                let now = std::time::SystemTime::now();

                if next_timestamp <= now {
                    break;
                }

                tokio::time::sleep(
                    next_timestamp
                        .duration_since(now)
                        .unwrap_or_else(|_| Duration::from_secs(0)),
                )
                .await;
                break;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    fn in_progress_executions(
        &self,
    ) -> impl Stream<Item = Result<Vec<InProgressExecution>>> + Send {
        async_stream::try_stream!({
            let mut last_execution_id: Option<ExecutionId> = None;

            loop {
                let mut conn = self.pool.get().await?;
                let tx = conn.read_only_transaction().await?;

                let rows = if let Some(last_execution_id) = last_execution_id {
                    let stmt = tx
                        .prepare(
                            r#"--sql
                            SELECT
                                ora.execution.id,
                                ora.job.id,
                                ora.execution.executor_id,
                                EXTRACT(EPOCH FROM ora.job.target_execution_time)::DOUBLE PRECISION,
                                EXTRACT(EPOCH FROM ora.execution.started_at)::DOUBLE PRECISION,
                                ora.job.timeout_policy_json,
                                ora.job.retry_policy_json,
                                (SELECT COUNT(*) FROM ora.execution ex WHERE ex.job_id = ora.job.id)
                            FROM
                                ora.execution
                            JOIN ora.job ON
                                ora.execution.job_id = ora.job.id
                            WHERE
                                status = 1
                                AND ora.execution.id > $1::UUID
                            ORDER BY
                                ora.execution.id ASC
                            LIMIT 1000
                            "#,
                        )
                        .await?;

                    tx.query(&stmt, &[&last_execution_id.0]).await?
                } else {
                    let stmt = tx
                        .prepare(
                            r#"--sql
                            SELECT
                                ora.execution.id,
                                ora.job.id,
                                ora.execution.executor_id,
                                EXTRACT(EPOCH FROM ora.job.target_execution_time)::DOUBLE PRECISION,
                                EXTRACT(EPOCH FROM ora.execution.started_at)::DOUBLE PRECISION,
                                ora.job.timeout_policy_json,
                                ora.job.retry_policy_json,
                                (SELECT COUNT(*) FROM ora.execution ex WHERE ex.job_id = ora.job.id)
                            FROM
                                ora.execution
                            JOIN ora.job ON
                                ora.execution.job_id = ora.job.id
                            WHERE
                                status = 1
                            ORDER BY
                                ora.execution.id ASC
                            LIMIT 1000
                        "#,
                        )
                        .await?;

                    tx.query(&stmt, &[]).await?
                };

                tx.commit().await?;

                if rows.is_empty() {
                    break;
                }

                let mut in_progress_executions = Vec::with_capacity(rows.len());

                for row in rows {
                    in_progress_executions.push(InProgressExecution {
                        execution_id: ExecutionId(row.try_get(0)?),
                        job_id: JobId(row.try_get(1)?),
                        executor_id: ExecutorId(row.try_get(2)?),
                        target_execution_time: UNIX_EPOCH
                            + std::time::Duration::from_secs_f64(row.try_get::<_, f64>(3)?),
                        started_at: UNIX_EPOCH
                            + std::time::Duration::from_secs_f64(row.try_get::<_, f64>(4)?),
                        timeout_policy: serde_json::from_str(&row.try_get::<_, String>(5)?)?,
                        retry_policy: serde_json::from_str(&row.try_get::<_, String>(6)?)?,
                        attempt_number: row.try_get::<_, i64>(7)? as u64,
                    });
                }

                last_execution_id = in_progress_executions.last().map(|e| e.execution_id);

                yield in_progress_executions;
            }
        })
    }

    async fn executions_started(&self, executions: &[StartedExecution]) -> Result<()> {
        if executions.is_empty() {
            return Ok(());
        }

        let mut conn = self.pool.get().await?;

        let tx = conn.transaction().await?;

        let stmt = tx
            .prepare(
                r#"--sql
                UPDATE ora.execution
                SET
                    executor_id = t.executor_id,
                    started_at = to_timestamp(t.started_at)
                FROM UNNEST(
                    $1::UUID[],
                    $2::UUID[],
                    $3::DOUBLE PRECISION[]
                ) AS t(execution_id, executor_id, started_at)
                WHERE
                    execution_id = id
                    AND ora.execution.started_at IS NULL
                "#,
            )
            .await?;

        let mut col_execution_id = Vec::with_capacity(executions.len());
        let mut col_executor_id = Vec::with_capacity(executions.len());
        let mut col_started_at = Vec::with_capacity(executions.len());

        for execution in executions {
            col_execution_id.push(execution.execution_id.0);
            col_executor_id.push(execution.executor_id.0);
            col_started_at.push(
                execution
                    .started_at
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
            );
        }

        tx.execute(
            &stmt,
            &[&col_execution_id, &col_executor_id, &col_started_at],
        )
        .await?;

        tx.commit().await?;

        Ok(())
    }

    async fn executions_succeeded(&self, executions: &[SucceededExecution]) -> Result<()> {
        if executions.is_empty() {
            return Ok(());
        }

        let mut conn = self.pool.get().await?;

        let tx = conn.transaction().await?;

        let stmt = tx
            .prepare(
                r#"--sql
                UPDATE ora.execution
                SET
                    succeeded_at = to_timestamp(t.succeeded_at),
                    output_json = t.output_json
                FROM UNNEST(
                    $1::UUID[],
                    $2::DOUBLE PRECISION[],
                    $3::TEXT[]
                ) AS t(execution_id, succeeded_at, output_json)
                WHERE
                    execution_id = id
                    AND status < 2
                RETURNING job_id, t.succeeded_at
                "#,
            )
            .await?;

        let mut col_execution_id = Vec::with_capacity(executions.len());
        let mut col_succeeded_at = Vec::with_capacity(executions.len());
        let mut col_output_json = Vec::with_capacity(executions.len());

        for execution in executions {
            col_execution_id.push(execution.execution_id.0);
            col_succeeded_at.push(
                execution
                    .succeeded_at
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
            );
            col_output_json.push(execution.output_json.as_str());
        }

        let rows = tx
            .query(
                &stmt,
                &[&col_execution_id, &col_succeeded_at, &col_output_json],
            )
            .await?;

        let mut jobs = Vec::with_capacity(rows.len());
        for row in rows {
            jobs.push((
                JobId(row.try_get(0)?),
                UNIX_EPOCH + std::time::Duration::from_secs_f64(row.try_get::<_, f64>(1)?),
            ));
        }

        mark_jobs_inactive(&tx, &jobs).await?;

        tx.commit().await?;

        Ok(())
    }

    async fn executions_failed(&self, executions: &[FailedExecution]) -> Result<()> {
        if executions.is_empty() {
            return Ok(());
        }

        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;

        let jobs = executions_failed(&tx, executions).await?;

        mark_jobs_inactive(&tx, &jobs).await?;

        tx.commit().await?;

        Ok(())
    }

    async fn executions_retried(&self, executions: &[FailedExecution]) -> Result<()> {
        if executions.is_empty() {
            return Ok(());
        }

        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;

        let jobs = executions_failed(&tx, executions).await?;
        add_executions(
            &tx,
            &jobs.into_iter().map(|(id, ..)| id).collect::<Vec<_>>(),
        )
        .await?;

        tx.commit().await?;

        Ok(())
    }

    fn pending_schedules(&self) -> impl Stream<Item = Result<Vec<PendingSchedule>>> + Send {
        async_stream::try_stream!({
            let mut last_schedule_id: Option<ScheduleId> = None;
            loop {
                let mut conn = self.pool.get().await?;
                let tx = conn.read_only_transaction().await?;

                let rows = if let Some(last_schedule_id) = last_schedule_id {
                    let stmt = tx
                        .prepare(
                            r#"--sql
                            SELECT
                                id,
                                EXTRACT(EPOCH FROM(
                                    SELECT
                                        target_execution_time
                                    FROM
                                        ora.job
                                    WHERE
                                        ora.job.schedule_id = ora.schedule.id
                                    ORDER BY ora.job.id DESC
                                    LIMIT 1
                                ))::DOUBLE PRECISION,
                                scheduling_policy_json,
                                EXTRACT(EPOCH FROM start_after)::DOUBLE PRECISION,
                                EXTRACT(EPOCH FROM end_before)::DOUBLE PRECISION,
                                job_template_json
                            FROM
                                ora.schedule
                            WHERE
                                (
                                    ora.schedule.stopped_at IS NULL
                                    AND ora.schedule.active_job_id IS NULL
                                )
                                AND id > $1::UUID
                            ORDER BY id ASC
                            LIMIT 1000
                            "#,
                        )
                        .await?;

                    tx.query(&stmt, &[&last_schedule_id.0]).await?
                } else {
                    let stmt = tx
                        .prepare(
                            r#"--sql
                            SELECT
                                id,
                                EXTRACT(EPOCH FROM(
                                    SELECT
                                        target_execution_time
                                    FROM
                                        ora.job
                                    WHERE
                                        ora.job.schedule_id = ora.schedule.id
                                    ORDER BY ora.job.id DESC
                                    LIMIT 1
                                ))::DOUBLE PRECISION,
                                scheduling_policy_json,
                                EXTRACT(EPOCH FROM start_after)::DOUBLE PRECISION,
                                EXTRACT(EPOCH FROM end_before)::DOUBLE PRECISION,
                                job_template_json
                            FROM
                                ora.schedule
                            WHERE
                                ora.schedule.stopped_at IS NULL
                                AND ora.schedule.active_job_id IS NULL
                            ORDER BY id ASC
                            LIMIT 1000
                            "#,
                        )
                        .await?;

                    tx.query(&stmt, &[]).await?
                };

                tx.commit().await?;

                if rows.is_empty() {
                    break;
                }

                let mut schedules = Vec::with_capacity(rows.len());

                for row in rows {
                    schedules.push(PendingSchedule {
                        schedule_id: ScheduleId(row.try_get(0)?),
                        last_target_execution_time: row
                            .try_get::<_, Option<f64>>(1)?
                            .map(|ts| UNIX_EPOCH + std::time::Duration::from_secs_f64(ts)),
                        scheduling: serde_json::from_str(row.try_get::<_, &str>(2)?)?,
                        time_range: TimeRange {
                            start: row
                                .try_get::<_, Option<f64>>(3)?
                                .map(|ts| UNIX_EPOCH + std::time::Duration::from_secs_f64(ts)),
                            end: row
                                .try_get::<_, Option<f64>>(4)?
                                .map(|ts| UNIX_EPOCH + std::time::Duration::from_secs_f64(ts)),
                        },
                        job_template: serde_json::from_str(row.try_get::<_, &str>(5)?)?,
                    });
                }

                last_schedule_id = schedules.last().map(|s| s.schedule_id);
                yield schedules;
            }
        })
    }

    async fn delete_history(&self, before: std::time::SystemTime) -> crate::Result<()> {
        let batch_size = i64::try_from(self.delete_batch_size).unwrap_or(i64::MAX);

        let mut conn = self.pool.get().await?;

        // delete in a loop so that we don't lock
        // tables for too long
        loop {
            let mut deleted_rows = 0;

            {
                let tx = conn.transaction().await?;

                let stmt = tx
                    .prepare(
                        r#"--sql
                        WITH cte AS (
                            SELECT ctid
                            FROM ora.job
                            WHERE inactive_since < to_timestamp($1)
                            ORDER BY inactive_since
                            LIMIT $2
                        )
                        DELETE FROM ora.job j
                        USING cte
                        WHERE j.ctid = cte.ctid;
                        "#,
                    )
                    .await?;

                let deleted = tx
                    .execute(
                        &stmt,
                        &[
                            &before
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs_f64(),
                            &batch_size,
                        ],
                    )
                    .await?;

                deleted_rows += deleted;

                tx.commit().await?;
            }

            {
                let tx = conn.transaction().await?;
                let stmt = tx
                    .prepare(
                        r#"--sql
                        WITH cte AS (
                            SELECT ctid
                            FROM ora.schedule
                            WHERE stopped_at < to_timestamp($1)
                            ORDER BY stopped_at
                            LIMIT $2
                        )
                        DELETE FROM ora.schedule s
                        USING cte
                        WHERE s.ctid = cte.ctid;
                        "#,
                    )
                    .await?;

                let deleted = tx
                    .execute(
                        &stmt,
                        &[
                            &before
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs_f64(),
                            &batch_size,
                        ],
                    )
                    .await?;

                tx.commit().await?;

                deleted_rows += deleted;
            }

            {
                let tx = conn.transaction().await?;

                let stmt = tx
                    .prepare(
                        r#"--sql
                        DELETE FROM ora.job_type
                        WHERE
                            NOT EXISTS (
                                SELECT FROM ora.job
                                WHERE
                                    ora.job.job_type_id = ora.job_type.id
                            )
                            AND NOT EXISTS (
                                SELECT FROM ora.schedule
                                WHERE
                                    ora.schedule.job_template_job_type_id = ora.job_type.id                                
                            )
                        "#,
                    )
                    .await?;

                tx.execute(&stmt, &[]).await?;

                tx.commit().await?;
            }

            if deleted_rows == 0 {
                break;
            }
        }

        Ok(())
    }

    async fn wait_for_pending_schedules(&self) -> crate::Result<()> {
        loop {
            let mut conn = self.pool.get().await?;

            let has_pending = {
                let tx = conn.read_only_transaction().await?;

                let stmt = tx
                    .prepare(
                        r#"--sql
                        SELECT 1 FROM ora.schedule
                        WHERE
                            ora.schedule.stopped_at IS NULL
                            AND ora.schedule.active_job_id IS NULL
                        LIMIT 1
                        "#,
                    )
                    .await?;

                let row = tx.query_opt(&stmt, &[]).await?;

                tx.commit().await?;

                row.is_some()
            };

            drop(conn);

            if has_pending {
                break;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }
}

async fn executions_failed(
    tx: &DbTransaction<'_>,
    executions: &[FailedExecution],
) -> Result<Vec<(JobId, SystemTime)>> {
    let stmt = tx
        .prepare(
            r#"--sql
            UPDATE ora.execution
            SET
                failed_at = to_timestamp(t.failed_at),
                failure_reason = t.failure_reason
            FROM UNNEST(
                $1::UUID[],
                $2::DOUBLE PRECISION[],
                $3::TEXT[]
            ) AS t(execution_id, failed_at, failure_reason)
            WHERE
                execution_id = id
                AND status < 2
            RETURNING job_id, t.failed_at
            "#,
        )
        .await?;

    let mut col_execution_id = Vec::with_capacity(executions.len());
    let mut col_succeeded_at = Vec::with_capacity(executions.len());
    let mut col_failure_reason = Vec::with_capacity(executions.len());

    for execution in executions {
        col_execution_id.push(execution.execution_id.0);
        col_succeeded_at.push(
            execution
                .failed_at
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        );
        col_failure_reason.push(execution.failure_reason.as_str());
    }

    let rows = tx
        .query(
            &stmt,
            &[&col_execution_id, &col_succeeded_at, &col_failure_reason],
        )
        .await?;

    let mut jobs = Vec::with_capacity(rows.len());

    for row in rows {
        jobs.push((
            JobId(row.try_get(0)?),
            UNIX_EPOCH + std::time::Duration::from_secs_f64(row.try_get::<_, f64>(1)?),
        ));
    }

    Ok(jobs)
}

async fn add_executions(tx: &DbTransaction<'_>, jobs: &[JobId]) -> Result<()> {
    if jobs.is_empty() {
        return Ok(());
    }

    let mut col_job_id = Vec::with_capacity(jobs.len());
    let mut col_execution_id = Vec::with_capacity(jobs.len());

    for job in jobs {
        col_job_id.push(job.0);
        col_execution_id.push(Uuid::now_v7());
    }

    let stmt = tx
        .prepare(
            r#"--sql
            INSERT INTO ora.execution (
                id,
                job_id
            ) SELECT * FROM UNNEST(
                $1::UUID[],
                $2::UUID[]
            )
            "#,
        )
        .await?;

    tx.execute(&stmt, &[&col_execution_id, &col_job_id]).await?;

    Ok(())
}

async fn mark_jobs_inactive(tx: &DbTransaction<'_>, jobs: &[(JobId, SystemTime)]) -> Result<()> {
    if jobs.is_empty() {
        return Ok(());
    }

    let mut col_job_id = Vec::with_capacity(jobs.len());
    let mut col_inactive_since = Vec::with_capacity(jobs.len());

    for (job, inactive_since) in jobs {
        col_job_id.push(job.0);
        col_inactive_since.push(
            inactive_since
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        );
    }

    {
        let stmt = tx
            .prepare(
                r#"--sql
                UPDATE ora.job
                SET inactive_since = to_timestamp(t.inactive_since)
                FROM
                    UNNEST(
                        $1::UUID[],
                        $2::DOUBLE PRECISION[]
                    ) AS t(job_id, inactive_since)
                WHERE
                    id = t.job_id;
                "#,
            )
            .await?;

        tx.execute(&stmt, &[&col_job_id, &col_inactive_since])
            .await?;
    }

    {
        let stmt = tx
            .prepare(
                r#"--sql
                UPDATE ora.schedule
                SET
                    active_job_id = NULL
                WHERE active_job_id = ANY($1::UUID[])
                "#,
            )
            .await?;

        tx.execute(&stmt, &[&col_job_id]).await?;
    }

    Ok(())
}
