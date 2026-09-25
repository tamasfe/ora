//! Postgres backend implementation for Ora.
#![allow(missing_docs)]

use std::{
    borrow::Cow,
    collections::HashSet,
    time::{Duration, SystemTime},
};

use deadpool_postgres::{Pool, PoolError};
use futures::Stream;
use ora_backend::{
    Backend,
    common::{NextPageToken, TimeRange},
    executions::{
        ExecutionId, ExecutionStatus, FailedExecution, InProgressExecution, ReadyExecution,
        RetriedExecution, StartedExecution, SucceededExecution,
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
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    db::{DbPool, DbTransaction},
    models::PgExecutionStatus,
    query::{
        jobs::{cancel_jobs, job_count},
        schedules::schedule_count,
    },
    util::{systemtime_from_ts, systemtime_to_ts},
};

embed_migrations!("./migrations");

mod db;
mod models;
mod query;
mod util;

/// Postgres backend for Ora.
#[must_use]
pub struct PostgresBackend {
    pool: DbPool,
    delete_batch_size: usize,
    poll_interval: Duration,
    new_executions: Notify,
    new_pending_schedules: Notify,
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
            poll_interval: Duration::from_millis(500),
            new_executions: Notify::new(),
            new_pending_schedules: Notify::new(),
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

    /// Set the poll interval for various waiting operations,
    /// such as pending executions.
    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    /// Wake up the waiters for ready executions if any of the
    /// new executions are due before their next poll,
    /// later executions are picked up by polling.
    fn notify_new_executions(&self, target_execution_times: impl IntoIterator<Item = SystemTime>) {
        let next_poll = SystemTime::now() + self.poll_interval;

        if target_execution_times.into_iter().any(|t| t < next_poll) {
            self.new_executions.notify_waiters();
        }
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
            // Concurrent calls would not see each other's jobs otherwise.
            lock_if_not_exists(&tx, IF_NOT_EXISTS_JOBS_LOCK).await?;

            let existing_job_ids = query::jobs::job_ids(&tx, filters).await?;

            if !existing_job_ids.is_empty() {
                tx.commit().await?;
                return Ok(AddedJobs::Existing(existing_job_ids));
            }
        }

        let job_ids = jobs.iter().map(|_| Uuid::now_v7()).collect::<Vec<_>>();

        // Jobs of schedules are only added if the schedule is still active
        // and has no active job, the schedule might have been stopped
        // or got a job since it was found to be pending.
        let claimed_job_ids = {
            let (col_job_id, col_schedule_id): (Vec<_>, Vec<_>) = jobs
                .iter()
                .zip(&job_ids)
                .filter_map(|(job, id)| Some((*id, job.schedule_id?.0)))
                .unzip();

            if col_job_id.is_empty() {
                HashSet::new()
            } else {
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
                            AND ora.schedule.stopped_at IS NULL
                            AND ora.schedule.active_job_id IS NULL
                        RETURNING t.job_id
                        "#,
                    )
                    .await?;

                tx.query(&stmt, &[&col_job_id, &col_schedule_id])
                    .await?
                    .into_iter()
                    .map(|row| row.try_get::<_, Uuid>(0))
                    .collect::<core::result::Result<HashSet<_>, _>>()?
            }
        };

        let jobs = jobs
            .iter()
            .zip(job_ids)
            .filter(|(job, id)| job.schedule_id.is_none() || claimed_job_ids.contains(id))
            .collect::<Vec<_>>();

        if jobs.is_empty() {
            tx.commit().await?;
            return Ok(AddedJobs::Added(Vec::new()));
        }

        let mut col_id = Vec::with_capacity(jobs.len());
        let mut new_executions = Vec::with_capacity(jobs.len());

        {
            let mut col_job_type_id = Vec::with_capacity(jobs.len());
            let mut col_target_execution_time = Vec::with_capacity(jobs.len());
            let mut col_input_payload_json = Vec::with_capacity(jobs.len());
            let mut col_timeout_policy_json = Vec::with_capacity(jobs.len());
            let mut col_retry_policy_json = Vec::with_capacity(jobs.len());
            let mut col_schedule_id = Vec::with_capacity(jobs.len());
            let mut col_priority = Vec::with_capacity(jobs.len());

            for (job, id) in &jobs {
                col_id.push(*id);
                col_job_type_id.push(job.job.job_type_id.as_str());
                col_target_execution_time.push(job.job.target_execution_time);
                col_input_payload_json.push(job.job.input_payload_json.as_str());
                col_timeout_policy_json.push(serde_json::to_string(&job.job.timeout_policy)?);
                col_retry_policy_json.push(serde_json::to_string(&job.job.retry_policy)?);
                col_schedule_id.push(job.schedule_id.map(|s| s.0));
                col_priority.push(job.job.priority);

                new_executions.push(NewExecution {
                    job_id: JobId(*id),
                    target_execution_time: job.job.target_execution_time,
                });
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
                        schedule_id,
                        priority
                    ) SELECT * FROM UNNEST(
                        $1::UUID[],
                        $2::TEXT[],
                        $3::TIMESTAMPTZ[],
                        $4::TEXT[],
                        $5::TEXT[],
                        $6::TEXT[],
                        $7::UUID[],
                        $8::INTEGER[]
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
                    &col_priority,
                ],
            )
            .await?;
        }

        {
            let mut col_job_id = Vec::with_capacity(jobs.len());
            let mut col_job_label_key = Vec::with_capacity(jobs.len());
            let mut col_job_label_value = Vec::with_capacity(jobs.len());

            for (job, id) in &jobs {
                for label in &job.job.labels {
                    col_job_id.push(*id);
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

        add_executions(&tx, &new_executions).await?;

        tx.commit().await?;

        self.notify_new_executions(new_executions.iter().map(|e| e.target_execution_time));

        Ok(AddedJobs::Added(
            col_id.into_iter().map(Into::into).collect::<Vec<_>>(),
        ))
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
        let schedules_freed = mark_jobs_inactive(
            &tx,
            &jobs.iter().map(|j| (j.job_id, now)).collect::<Vec<_>>(),
            ExecutionStatus::Cancelled,
        )
        .await?;
        tx.commit().await?;

        if schedules_freed {
            self.new_pending_schedules.notify_waiters();
        }

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
            // Concurrent calls would not see each other's schedules otherwise.
            lock_if_not_exists(&tx, IF_NOT_EXISTS_SCHEDULES_LOCK).await?;

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

        self.new_pending_schedules.notify_waiters();

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

    fn ready_executions(
        &self,
        ignore: &[ExecutionId],
    ) -> impl Stream<Item = Result<Vec<ReadyExecution>>> + Send {
        let ignored_executions = ignore.iter().map(|id| id.0).collect::<Vec<_>>();

        async_stream::try_stream!({
            let mut last_execution: Option<(i32, ExecutionId)> = None;

            loop {
                let mut conn = self.pool.get().await?;
                let tx = conn.read_only_transaction().await?;

                let rows = if let Some((last_priority, last_execution_id)) = last_execution {
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
                                EXTRACT(EPOCH FROM ora.execution.target_execution_time)::DOUBLE PRECISION,
                                ora.execution.priority
                            FROM
                                ora.execution
                            JOIN ora.job ON
                                ora.execution.job_id = ora.job.id
                            WHERE
                                status = 0
                                AND ora.execution.target_execution_time <= NOW()
                                AND ora.execution.id <> ALL($1::UUID[])
                                -- The priority is negated so that the ordering matches
                                -- the index, it is cast to avoid overflows.
                                AND (-ora.execution.priority::BIGINT, ora.execution.id)
                                    > (-$2::INTEGER::BIGINT, $3::UUID)
                            ORDER BY
                                -ora.execution.priority::BIGINT ASC,
                                ora.execution.id ASC
                            LIMIT 1000
                            "#,
                        )
                        .await?;

                    tx.query(
                        &stmt,
                        &[&ignored_executions, &last_priority, &last_execution_id.0],
                    )
                    .await?
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
                                EXTRACT(EPOCH FROM ora.execution.target_execution_time)::DOUBLE PRECISION,
                                ora.execution.priority
                            FROM
                                ora.execution
                            JOIN ora.job ON
                                ora.execution.job_id = ora.job.id
                            WHERE
                                status = 0
                                AND ora.execution.target_execution_time <= NOW()
                                AND ora.execution.id <> ALL($1::UUID[])
                            ORDER BY
                                -ora.execution.priority::BIGINT ASC,
                                ora.execution.id ASC
                            LIMIT 1000
                            "#,
                        )
                        .await?;

                    tx.query(&stmt, &[&ignored_executions]).await?
                };

                tx.commit().await?;

                // The connection must not be held while the consumer
                // processes the batch, it might need connections as well.
                drop(conn);

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
                        target_execution_time: systemtime_from_ts(row.try_get::<_, f64>(6)?),
                        priority: row.try_get(7)?,
                    });
                }

                last_execution = ready_executions
                    .last()
                    .map(|e| (e.priority, e.execution_id));

                yield ready_executions;
            }
        })
    }

    async fn wait_for_ready_executions(&self, ignore: &[ExecutionId]) -> crate::Result<()> {
        let ignored_executions = ignore.iter().map(|id| id.0).collect::<Vec<_>>();

        loop {
            // Created before the query so that executions
            // added after the query still wake us up.
            let new_executions = self.new_executions.notified();

            let mut conn = self.pool.get().await?;

            let seconds_until_next = {
                let tx = conn.read_only_transaction().await?;

                // The remaining time is determined by the database clock,
                // it decides which executions are ready.
                let stmt = tx
                    .prepare(
                        r#"--sql
                        SELECT
                            EXTRACT(EPOCH FROM (target_execution_time - NOW()))::DOUBLE PRECISION
                        FROM
                            ora.execution
                        WHERE
                            ora.execution.status = 0
                            AND NOT (ora.execution.id = ANY($1::UUID[]))
                        ORDER BY
                            ora.execution.target_execution_time ASC
                        LIMIT 1
                        "#,
                    )
                    .await?;

                let row = tx.query_opt(&stmt, &[&ignored_executions]).await?;

                tx.commit().await?;

                match row {
                    Some(row) => row.try_get::<_, Option<f64>>(0)?,
                    None => None,
                }
            };

            drop(conn);

            // Never wait longer than the poll interval,
            // `notify_new_executions` relies on it.
            let mut delay = self.poll_interval;

            if let Some(seconds_until_next) = seconds_until_next {
                if seconds_until_next <= 0.0 {
                    break;
                }

                if let Ok(until_next) = Duration::try_from_secs_f64(seconds_until_next) {
                    delay = delay.min(until_next);
                }
            }

            tokio::select! {
                () = new_executions => break,
                () = tokio::time::sleep(delay) => {}
            }
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

                // The connection must not be held while the consumer
                // processes the batch, it might need connections as well.
                drop(conn);

                if rows.is_empty() {
                    break;
                }

                let mut in_progress_executions = Vec::with_capacity(rows.len());

                for row in rows {
                    in_progress_executions.push(InProgressExecution {
                        execution_id: ExecutionId(row.try_get(0)?),
                        job_id: JobId(row.try_get(1)?),
                        executor_id: ExecutorId(row.try_get(2)?),
                        target_execution_time: systemtime_from_ts(row.try_get::<_, f64>(3)?),
                        started_at: systemtime_from_ts(row.try_get::<_, f64>(4)?),
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

    async fn executions_started(
        &self,
        executions: &[StartedExecution],
    ) -> Result<Vec<ExecutionId>> {
        if executions.is_empty() {
            return Ok(Vec::new());
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
                    AND (
                        ora.execution.status = 0
                        -- Repeated calls (e.g. retries) return the same executions.
                        OR (
                            ora.execution.status = 1
                            AND ora.execution.executor_id = t.executor_id
                        )
                    )
                RETURNING id
                "#,
            )
            .await?;

        let mut col_execution_id = Vec::with_capacity(executions.len());
        let mut col_executor_id = Vec::with_capacity(executions.len());
        let mut col_started_at = Vec::with_capacity(executions.len());

        for execution in executions {
            col_execution_id.push(execution.execution_id.0);
            col_executor_id.push(execution.executor_id.0);
            col_started_at.push(systemtime_to_ts(execution.started_at));
        }

        let rows = tx
            .query(
                &stmt,
                &[&col_execution_id, &col_executor_id, &col_started_at],
            )
            .await?;

        tx.commit().await?;

        rows.into_iter()
            .map(|row| Ok(ExecutionId(row.try_get(0)?)))
            .collect()
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
            col_succeeded_at.push(systemtime_to_ts(execution.succeeded_at));
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
                systemtime_from_ts(row.try_get::<_, f64>(1)?),
            ));
        }

        let schedules_freed = mark_jobs_inactive(&tx, &jobs, ExecutionStatus::Succeeded).await?;

        tx.commit().await?;

        if schedules_freed {
            self.new_pending_schedules.notify_waiters();
        }

        Ok(())
    }

    async fn executions_failed(&self, executions: &[FailedExecution]) -> Result<()> {
        if executions.is_empty() {
            return Ok(());
        }

        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;

        let jobs = executions_failed(&tx, executions).await?;

        let schedules_freed = mark_jobs_inactive(&tx, &jobs, ExecutionStatus::Failed).await?;

        tx.commit().await?;

        if schedules_freed {
            self.new_pending_schedules.notify_waiters();
        }

        Ok(())
    }

    async fn executions_retried(&self, executions: &[RetriedExecution]) -> Result<()> {
        if executions.is_empty() {
            return Ok(());
        }

        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;

        let mut failed_executions = Vec::with_capacity(executions.len());
        let mut new_executions = Vec::with_capacity(executions.len());

        for execution in executions {
            failed_executions.push(FailedExecution {
                execution_id: execution.failed_execution.execution_id,
                job_id: execution.failed_execution.job_id,
                failed_at: execution.failed_execution.failed_at,
                failure_reason: execution.failed_execution.failure_reason.clone(),
            });

            new_executions.push(NewExecution {
                job_id: execution.failed_execution.job_id,
                target_execution_time: execution.retry_execution_time,
            });
        }

        let jobs_active = executions_failed(&tx, &failed_executions).await?;

        // Just an additional check against race conditions,
        // we must only retry jobs that are still active according to the db state.
        new_executions.retain(|n| jobs_active.iter().any(|(j, _)| j == &n.job_id));

        if !new_executions.is_empty() {
            add_executions(&tx, &new_executions).await?;
        }

        tx.commit().await?;

        self.notify_new_executions(new_executions.iter().map(|e| e.target_execution_time));

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

                // The connection must not be held while the consumer
                // processes the batch, it might need connections as well.
                drop(conn);

                if rows.is_empty() {
                    break;
                }

                let mut schedules = Vec::with_capacity(rows.len());

                for row in rows {
                    schedules.push(PendingSchedule {
                        schedule_id: ScheduleId(row.try_get(0)?),
                        last_target_execution_time: row
                            .try_get::<_, Option<f64>>(1)?
                            .map(systemtime_from_ts),
                        scheduling: serde_json::from_str(row.try_get::<_, &str>(2)?)?,
                        time_range: TimeRange {
                            start: row.try_get::<_, Option<f64>>(3)?.map(systemtime_from_ts),
                            end: row.try_get::<_, Option<f64>>(4)?.map(systemtime_from_ts),
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
                    .execute(&stmt, &[&systemtime_to_ts(before), &batch_size])
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
                    .execute(&stmt, &[&systemtime_to_ts(before), &batch_size])
                    .await?;

                tx.commit().await?;

                deleted_rows += deleted;
            }

            if deleted_rows == 0 {
                break;
            }
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

        Ok(())
    }

    async fn wait_for_pending_schedules(&self) -> crate::Result<()> {
        loop {
            // Created before the query so that schedules
            // becoming pending after the query still wake us up.
            let new_pending_schedules = self.new_pending_schedules.notified();

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

            tokio::select! {
                () = new_pending_schedules => break,
                () = tokio::time::sleep(self.poll_interval) => {}
            }
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
    let mut col_failed_at = Vec::with_capacity(executions.len());
    let mut col_failure_reason = Vec::with_capacity(executions.len());

    for execution in executions {
        col_execution_id.push(execution.execution_id.0);
        col_failed_at.push(systemtime_to_ts(execution.failed_at));
        // Text can not contain NUL characters in Postgres,
        // failure reasons are arbitrary and must not fail the update.
        col_failure_reason.push(if execution.failure_reason.contains('\0') {
            Cow::Owned(execution.failure_reason.replace('\0', "\u{FFFD}"))
        } else {
            Cow::Borrowed(execution.failure_reason.as_str())
        });
    }

    let rows = tx
        .query(
            &stmt,
            &[&col_execution_id, &col_failed_at, &col_failure_reason],
        )
        .await?;

    let mut jobs = Vec::with_capacity(rows.len());

    for row in rows {
        jobs.push((
            JobId(row.try_get(0)?),
            systemtime_from_ts(row.try_get::<_, f64>(1)?),
        ));
    }

    Ok(jobs)
}

/// Advisory lock key serializing `add_jobs` calls with `if_not_exists`.
const IF_NOT_EXISTS_JOBS_LOCK: i64 = 0x6f72_615f_6a6f_6273; // "ora_jobs"
/// Advisory lock key serializing `add_schedules` calls with `if_not_exists`.
const IF_NOT_EXISTS_SCHEDULES_LOCK: i64 = 0x6f72_615f_7363_6864; // "ora_schd"

/// Take a transaction-level advisory lock, so that
/// existence checks and inserts are not interleaved
/// with concurrent transactions doing the same.
async fn lock_if_not_exists(tx: &DbTransaction<'_>, key: i64) -> Result<()> {
    let stmt = tx
        .prepare(
            r#"--sql
            SELECT pg_advisory_xact_lock($1::BIGINT)
            "#,
        )
        .await?;

    tx.execute(&stmt, &[&key]).await?;

    Ok(())
}

struct NewExecution {
    job_id: JobId,
    target_execution_time: SystemTime,
}

async fn add_executions(tx: &DbTransaction<'_>, executions: &[NewExecution]) -> Result<()> {
    if executions.is_empty() {
        return Ok(());
    }

    let mut col_execution_id = Vec::with_capacity(executions.len());
    let mut col_job_id = Vec::with_capacity(executions.len());
    let mut col_target_execution_time = Vec::with_capacity(executions.len());

    for NewExecution {
        job_id,
        target_execution_time,
    } in executions
    {
        col_execution_id.push(Uuid::now_v7());
        col_job_id.push(job_id.0);
        col_target_execution_time.push(systemtime_to_ts(*target_execution_time));
    }

    let stmt = tx
        .prepare(
            r#"--sql
            INSERT INTO ora.execution (
                id,
                job_id,
                target_execution_time,
                priority
            ) SELECT
                t.execution_id,
                t.job_id,
                to_timestamp(t.target_execution_time),
                ora.job.priority
            FROM UNNEST(
                $1::UUID[],
                $2::UUID[],
                $3::DOUBLE PRECISION[]
            ) as t(execution_id, job_id, target_execution_time)
            -- The priority is copied from the job so that
            -- ready executions can be ordered efficiently.
            JOIN ora.job ON ora.job.id = t.job_id
            "#,
        )
        .await?;

    tx.execute(
        &stmt,
        &[&col_execution_id, &col_job_id, &col_target_execution_time],
    )
    .await?;

    Ok(())
}

/// Marks the jobs as finished with the given status.
///
/// Returns whether any schedules were left without an active job.
async fn mark_jobs_inactive(
    tx: &DbTransaction<'_>,
    jobs: &[(JobId, SystemTime)],
    status: ExecutionStatus,
) -> Result<bool> {
    if jobs.is_empty() {
        return Ok(false);
    }

    let mut col_job_id = Vec::with_capacity(jobs.len());
    let mut col_inactive_since = Vec::with_capacity(jobs.len());

    for (job, inactive_since) in jobs {
        col_job_id.push(job.0);
        col_inactive_since.push(systemtime_to_ts(*inactive_since));
    }

    {
        let stmt = tx
            .prepare(
                r#"--sql
                UPDATE ora.job
                SET
                    inactive_since = to_timestamp(t.inactive_since),
                    inactive_status = $3::SMALLINT
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

        tx.execute(
            &stmt,
            &[
                &col_job_id,
                &col_inactive_since,
                &(PgExecutionStatus::from(status) as i16),
            ],
        )
        .await?;
    }

    let rows_affected = {
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

        tx.execute(&stmt, &[&col_job_id]).await?
    };

    Ok(rows_affected > 0)
}
