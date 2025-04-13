//! Storage implementation for `ora` backed by sqlite.

use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use eyre::Context;
use migrations::run_migrations;
use models::{JobRetryPolicy, JobTimeoutPolicy, SqlSystemTime};
use ora_storage::{ScheduleTimeRange, Storage};
use parking_lot::Mutex;
use rusqlite::{named_params, params, params_from_iter, Connection, OpenFlags};
use uuid::Uuid;

#[macro_use]
mod util;

mod migrations;
mod models;

mod job_query;
mod schedule_query;

mod sea_query_binder;

const DEFAULT_TEMP_TABLE_THRESHOLD: usize = 1000;

/// A storage implementation backed by sqlite.
#[derive(Debug, Clone)]
pub struct SqliteStorage {
    /// The sqlite connection pool.
    pool: deadpool::unmanaged::Pool<Connection>,
    /// A mutex that ensures that scheduler operations
    /// are always ran sequentially.
    scheduler_mutex: Arc<Mutex<()>>,
}

/// Configuration for the sqlite storage.
#[must_use]
pub struct SqliteStorageConfig {
    /// The path to the sqlite database file.
    ///
    /// If not provided, a temporary in-memory database will be used.
    path: Option<PathBuf>,
    /// Flags that are used to configure the sqlite database.
    flags: OpenFlags,
    /// A function that is called whenever a new connection is created.
    /// This can be used to set PRAGMA options on the connection.
    #[allow(clippy::type_complexity)]
    conn_init: Option<Box<dyn Fn(&mut Connection) -> eyre::Result<()>>>,
    /// A function that is called whenever the first connection is created.
    #[allow(clippy::type_complexity)]
    init: Option<Box<dyn Fn(&mut Connection) -> eyre::Result<()>>>,
    /// The size of the connection pool.
    ///
    /// If not provided, a default size of 1 will be used.
    size: usize,
}

impl SqliteStorageConfig {
    /// Create a new `SqliteStorageConfig` with the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            conn_init: None,
            init: None,
            flags: OpenFlags::default(),
            size: 1,
        }
    }

    /// Create a new `SqliteStorageConfig` with an in-memory database.
    pub fn new_in_memory() -> Self {
        Self {
            path: None,
            conn_init: None,
            init: None,
            flags: OpenFlags::default(),
            size: 1,
        }
    }

    /// Set the function that is called whenever the first connection is created.
    ///
    /// Note that only one function can be set at a time,
    /// so this will overwrite any existing function.
    ///
    /// Useful for setting PRAGMA options that affect the entire database
    /// and all connections.
    pub fn with_init<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut Connection) -> eyre::Result<()> + 'static,
    {
        self.init = Some(Box::new(f));
        self
    }

    /// Set the function that is called whenever a new connection is created.
    ///
    /// Note that only one function can be set at a time,
    /// so this will overwrite any existing function.
    pub fn with_connection_init<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut Connection) -> eyre::Result<()> + 'static,
    {
        self.conn_init = Some(Box::new(f));
        self
    }

    /// Set the flags that are used to configure the sqlite database.
    pub fn with_flags(mut self, flags: OpenFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Set the amount of connections to use to access the database.
    ///
    /// The connections are created eagerly when the storage is created.
    ///
    /// # Panics
    ///
    /// Panics if the size is 0.
    pub fn with_connection_count(mut self, size: usize) -> Self {
        assert!(size > 0, "pool size must be greater than 0");
        self.size = size;
        self
    }

    /// Create a new connection.
    fn new_connection(&self) -> rusqlite::Result<Connection> {
        tracing::debug!("creating new sqlite connection");
        if let Some(path) = &self.path {
            Connection::open_with_flags(path, self.flags)
        } else {
            Connection::open_in_memory_with_flags(self.flags)
        }
    }
}

impl std::fmt::Debug for SqliteStorageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStorageConfig")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl SqliteStorage {
    /// Create a new sqlite storage instance with the given connection.
    pub fn new(mut config: SqliteStorageConfig) -> eyre::Result<Self> {
        if config.path.is_none() && config.size > 1 {
            tracing::warn!("sqlite in-memory storage with multiple connections is not supported");
            config.size = 1;
        }

        let pool = deadpool::unmanaged::Pool::new(config.size);

        for i in 0..config.size {
            let mut conn = config.new_connection()?;

            if i == 0 {
                if let Some(init) = &config.init {
                    init(&mut conn).wrap_err("failed to initialize sqlite connection")?;
                }

                if let Some(conn_init) = &config.conn_init {
                    conn_init(&mut conn).wrap_err("failed to initialize sqlite connection")?;
                }

                run_migrations(&mut conn).wrap_err("failed to run migrations")?;
            } else if let Some(conn_init) = &config.conn_init {
                conn_init(&mut conn).wrap_err("failed to initialize sqlite connection")?;
            }

            pool.try_add(conn).map_err(|(_, e)| e)?;
        }

        Ok(Self {
            pool,
            scheduler_mutex: Arc::new(Mutex::new(())),
        })
    }

    /// Run `optimize` on the database.
    ///
    /// This should be called periodically (e.g. hourly).
    pub async fn optimize(&self) -> eyre::Result<()> {
        self.with_db(|db| {
            db.execute_batch(
                r#"--sql
                    PRAGMA optimize;
                "#,
            )?;
            Ok(())
        })
        .await
    }

    /// Run `vacuum` on the database.
    ///
    /// This should be called periodically depending
    /// on the amount of data generated and deleted (e.g. daily).
    pub async fn vacuum(&self) -> eyre::Result<()> {
        self.with_db(|db| {
            db.execute_batch(
                r#"--sql
                    VACUUM;
                "#,
            )?;
            Ok(())
        })
        .await
    }

    /// Run a function on a separate thread
    /// with a mutable reference to an sqlite connection.
    async fn with_db_concurrent<F, O>(&self, f: F) -> eyre::Result<O>
    where
        F: FnOnce(&mut Connection) -> eyre::Result<O> + Send + 'static,
        O: Send + 'static,
    {
        let span = tracing::Span::current();

        tracing::trace!("acquiring database connection");
        let mut conn = self.pool.get().await?;
        tracing::trace!("acquired database connection");

        tokio::task::spawn_blocking(move || {
            let _guard = span.enter();
            f(&mut conn)
        })
        .await
        .unwrap()
    }

    /// Run a function on a separate thread
    /// with a mutable reference to an sqlite connection.
    ///
    /// This function is used to run scheduler operations
    /// that need to be run sequentially.
    async fn with_db<F, O>(&self, f: F) -> eyre::Result<O>
    where
        F: FnOnce(&mut Connection) -> eyre::Result<O> + Send + 'static,
        O: Send + 'static,
    {
        let scheduler_mutex = self.scheduler_mutex.clone();
        self.with_db_concurrent(move |conn| {
            let _guard = scheduler_mutex.lock();
            f(conn)
        })
        .await
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    #[tracing::instrument(skip_all)]
    async fn job_types_added(&self, job_types: Vec<ora_storage::JobType>) -> eyre::Result<()> {
        if job_types.is_empty() {
            return Ok(());
        }

        self.with_db(|db| {
            let tx = db.transaction()?;

            {
                let mut insert_stmt = tx.prepare(
                    r#"--sql
                        INSERT INTO ora_job_type (
                            id,
                            name,
                            description,
                            input_schema_json,
                            output_schema_json
                        )
                        VALUES (
                            :id,
                            :name,
                            :description,
                            :input_schema_json,
                            :output_schema_json
                        )
                        ON CONFLICT(id) DO UPDATE SET
                            name = excluded.name,
                            description = excluded.description,
                            input_schema_json = excluded.input_schema_json,
                            output_schema_json = excluded.output_schema_json;
                    "#,
                )?;

                for job_type in job_types {
                    insert_stmt.execute(params![
                        job_type.id,
                        job_type.name,
                        job_type.description,
                        job_type.input_schema_json,
                        job_type.output_schema_json
                    ])?;
                }
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn jobs_added(&self, jobs: Vec<ora_storage::NewJob>) -> eyre::Result<()> {
        if jobs.is_empty() {
            return Ok(());
        }

        self.with_db(|db| {
            let tx = db.transaction()?;

            {
                let mut insert_stmt = tx.prepare_cached(
                    r#"--sql
                        INSERT INTO ora_job (
                            id,
                            schedule_id,
                            created_at_unix_ns,
                            job_type_id,
                            target_execution_time_unix_ns,
                            retry_policy,
                            timeout_policy,
                            input_payload_json,
                            metadata_json
                        )
                        VALUES (
                            :id,
                            :schedule_id,
                            :created_at_unix_ns,
                            :job_type_id,
                            :target_execution_time_unix_ns,
                            :retry_policy,
                            :timeout_policy,
                            :input_payload_json,
                            :metadata_json
                        );
                    "#,
                )?;

                let mut insert_label_stmt = tx.prepare_cached(
                    r#"--sql
                        INSERT INTO ora_job_label (
                            job_id,
                            key,
                            value
                        )
                        VALUES (
                            :job_id,
                            :key,
                            :value
                        );
                    "#,
                )?;

                for job in jobs {
                    insert_stmt.execute(params![
                        job.id,
                        job.schedule_id,
                        SqlSystemTime(job.created_at),
                        job.job_type_id,
                        SqlSystemTime(job.target_execution_time),
                        JobRetryPolicy::from(job.retry_policy),
                        JobTimeoutPolicy::from(job.timeout_policy),
                        job.input_payload_json,
                        job.metadata_json
                    ])?;

                    for (key, value) in job.labels {
                        insert_label_stmt.execute(params![job.id, key, value])?;
                    }
                }
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn jobs_cancelled(
        &self,
        job_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<Vec<ora_storage::CancelledJob>> {
        const TEMP_TABLE_THRESHOLD: usize = DEFAULT_TEMP_TABLE_THRESHOLD;

        if job_ids.is_empty() {
            return Ok(vec![]);
        }

        let job_ids: Vec<_> = job_ids.into();

        self.with_db(move |db| {
            let tx = db.transaction()?;

            let cancelled_jobs = if job_ids.len() < TEMP_TABLE_THRESHOLD {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_job
                        SET
                            cancelled_at_unix_ns = :cancelled_at_unix_ns,
                            marked_unschedulable_at_unix_ns = :cancelled_at_unix_ns
                        WHERE
                            id = :id
                            AND cancelled_at_unix_ns IS NULL
                            AND marked_unschedulable_at_unix_ns IS NULL
                        RETURNING id, (
                            SELECT id
                            FROM ora_execution
                            WHERE job_id = ora_job.id
                            AND active
                            LIMIT 1
                        ) AS execution_id;
                    "#,
                )?;

                let mut cancelled_jobs = Vec::new();

                for id in &job_ids {
                    let mut rows = update_stmt.query_map(
                        named_params![
                            ":cancelled_at_unix_ns": SqlSystemTime(timestamp),
                            ":id": id
                        ],
                        |row| {
                            Ok(ora_storage::CancelledJob {
                                id: row.get(0)?,
                                active_execution: row.get(1)?,
                            })
                        },
                    )?;

                    if let Some(cancelled_job) = rows.next() {
                        let cancelled_job = cancelled_job?;
                        cancelled_jobs.push(cancelled_job);
                    }
                }

                cancelled_jobs
            } else {
                {
                    tx.execute("CREATE TABLE temp.job_ids (id BLOB)", [])?;

                    let mut insert_stmt = tx.prepare("INSERT INTO temp.job_ids (id) VALUES (?)")?;

                    for id in &job_ids {
                        insert_stmt.execute(params![id])?;
                    }
                }

                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                    UPDATE ora_job
                    SET
                        cancelled_at_unix_ns = :cancelled_at_unix_ns,
                        marked_unschedulable_at_unix_ns = :cancelled_at_unix_ns
                    WHERE
                        id IN (SELECT id FROM temp.job_ids)
                        AND cancelled_at_unix_ns IS NULL
                        AND marked_unschedulable_at_unix_ns IS NULL
                    RETURNING id, (
                        SELECT id
                        FROM ora_execution
                        WHERE job_id = ora_job.id
                        AND active
                        LIMIT 1
                    ) AS execution_id;
                    "#,
                )?;

                let cancelled_jobs = update_stmt
                    .query_map(
                        named_params![
                            ":cancelled_at_unix_ns": SqlSystemTime(timestamp)
                        ],
                        |row| {
                            Ok(ora_storage::CancelledJob {
                                id: row.get(0)?,
                                active_execution: row.get(1)?,
                            })
                        },
                    )?
                    .collect::<Result<Vec<ora_storage::CancelledJob>, _>>()?;

                tx.execute("DROP TABLE temp.job_ids", [])?;

                cancelled_jobs
            };

            tx.commit()?;

            Ok(cancelled_jobs)
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn executions_added(
        &self,
        executions: Vec<ora_storage::NewExecution>,
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        if executions.is_empty() {
            return Ok(());
        }

        self.with_db(move |db| {
            let tx = db.transaction()?;

            {
                let mut insert_stmt = tx.prepare_cached(
                    r#"--sql
                        INSERT INTO ora_execution (
                            id,
                            job_id,
                            created_at_unix_ns
                        )
                        VALUES (
                            :id,
                            :job_id,
                            :created_at_unix_ns
                        );
                    "#,
                )?;

                for execution in executions {
                    insert_stmt.execute(params![
                        execution.id,
                        execution.job_id,
                        SqlSystemTime(timestamp)
                    ])?;
                }
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn executions_ready(
        &self,
        execution_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        const TEMP_TABLE_THRESHOLD: usize = DEFAULT_TEMP_TABLE_THRESHOLD;

        if execution_ids.is_empty() {
            return Ok(());
        }

        let execution_ids: Vec<_> = execution_ids.into();

        self.with_db(move |db| {
            let tx = db.transaction()?;

            if execution_ids.len() < TEMP_TABLE_THRESHOLD {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_execution
                        SET ready_at_unix_ns = :ready_at_unix_ns
                        WHERE id = :id;
                    "#,
                )?;

                for id in &execution_ids {
                    update_stmt.execute(params![SqlSystemTime(timestamp), id])?;
                }
            } else {
                {
                    tx.execute("CREATE TABLE temp.execution_ids (id BLOB)", [])?;

                    let mut insert_stmt =
                        tx.prepare("INSERT INTO temp.execution_ids (id) VALUES (?)")?;

                    for id in &execution_ids {
                        insert_stmt.execute(params![id])?;
                    }
                }

                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_execution
                        SET ready_at_unix_ns = :ready_at_unix_ns
                        WHERE
                            id IN (SELECT id FROM temp.execution_ids)
                            AND ready_at_unix_ns IS NULL;
                    "#,
                )?;

                update_stmt.execute(params![SqlSystemTime(timestamp)])?;

                tx.execute("DROP TABLE IF EXISTS temp.execution_ids", [])?;
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn execution_assigned(
        &self,
        execution_id: Uuid,
        executor_id: Uuid,
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        self.with_db(move |db| {
            let tx = db.transaction()?;

            {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_execution
                        SET executor_id = :executor_id,
                            assigned_at_unix_ns = :assigned_at_unix_ns
                        WHERE id = :id;
                    "#,
                )?;

                update_stmt.execute(named_params![
                    ":executor_id": executor_id,
                    ":assigned_at_unix_ns": SqlSystemTime(timestamp),
                    ":id": execution_id
                ])?;
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn execution_started(
        &self,
        execution_id: Uuid,
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        self.with_db(move |db| {
            let tx = db.transaction()?;

            {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_execution
                        SET started_at_unix_ns = :started_at_unix_ns
                        WHERE id = :id;
                    "#,
                )?;

                update_stmt.execute(named_params! {
                    ":started_at_unix_ns": SqlSystemTime(timestamp),
                    ":id": execution_id
                })?;
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn execution_succeeded(
        &self,
        execution_id: Uuid,
        timestamp: std::time::SystemTime,
        output_payload_json: String,
    ) -> eyre::Result<()> {
        self.with_db(move |db| {
            let tx = db.transaction()?;

            {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_execution
                        SET succeeded_at_unix_ns = :succeeded_at_unix_ns,
                            output_payload_json = :output_payload_json
                        WHERE id = :id;
                        "#,
                )?;

                update_stmt.execute(named_params![
                    ":succeeded_at_unix_ns": SqlSystemTime(timestamp),
                    ":output_payload_json": output_payload_json,
                    ":id": execution_id
                ])?;
            }

            {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                    UPDATE ora_job
                    SET marked_unschedulable_at_unix_ns = :marked_unschedulable_at_unix_ns
                    WHERE id = (SELECT job_id FROM ora_execution WHERE id = :id);
                    "#,
                )?;

                update_stmt.execute(named_params! {
                    ":marked_unschedulable_at_unix_ns": SqlSystemTime(timestamp),
                    ":id": execution_id
                })?;
            }
            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn executions_failed(
        &self,
        execution_ids: &[Uuid],
        timestamp: std::time::SystemTime,
        reason: String,
        mark_job_unschedulable: bool,
    ) -> eyre::Result<()> {
        const TEMP_TABLE_THRESHOLD: usize = DEFAULT_TEMP_TABLE_THRESHOLD;

        if execution_ids.is_empty() {
            return Ok(());
        }

        let execution_ids: Vec<_> = execution_ids.into();

        self.with_db(move |db| {
            let tx = db.transaction()?;

            if execution_ids.len() < TEMP_TABLE_THRESHOLD {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_execution
                        SET failed_at_unix_ns = :failed_at_unix_ns,
                            failure_reason = :failure_reason
                        WHERE id = :id;
                    "#,
                )?;

                for id in &execution_ids {
                    update_stmt.execute(named_params![
                        ":failed_at_unix_ns": SqlSystemTime(timestamp),
                        ":failure_reason": reason,
                        ":id": id
                    ])?;
                }

                if mark_job_unschedulable {
                    let mut update_stmt = tx.prepare_cached(
                        r#"--sql
                            UPDATE ora_job
                            SET marked_unschedulable_at_unix_ns = :marked_unschedulable_at_unix_ns
                            WHERE id = (SELECT job_id FROM ora_execution WHERE id = :id);
                        "#,
                    )?;

                    for id in &execution_ids {
                        update_stmt.execute(named_params! {
                            ":marked_unschedulable_at_unix_ns": SqlSystemTime(timestamp),
                            ":id": id
                        })?;
                    }
                }
            } else {
                {
                    tx.execute("CREATE TABLE temp.execution_ids (id BLOB)", [])?;

                    let mut insert_stmt =
                        tx.prepare("INSERT INTO temp.execution_ids (id) VALUES (?)")?;

                    for id in &execution_ids {
                        insert_stmt.execute(params![id])?;
                    }
                }

                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_execution
                        SET failed_at_unix_ns = :failed_at_unix_ns,
                            failure_reason = :failure_reason
                        WHERE
                            id IN (SELECT id FROM temp.execution_ids)
                            AND failed_at_unix_ns IS NULL;
                    "#,
                )?;

                update_stmt.execute(named_params![
                    ":failed_at_unix_ns": SqlSystemTime(timestamp),
                    ":failure_reason": reason
                ])?;

                if mark_job_unschedulable {
                    let mut update_stmt = tx.prepare_cached(
                        r#"--sql
                            UPDATE ora_job
                            SET marked_unschedulable_at_unix_ns = :marked_unschedulable_at_unix_ns
                            WHERE id IN (SELECT job_id FROM ora_execution WHERE id IN (SELECT id FROM temp.execution_ids));
                        "#,
                    )?;

                    update_stmt.execute(named_params! {
                        ":marked_unschedulable_at_unix_ns": SqlSystemTime(timestamp)
                    })?;
                }
            }



            tx.execute("DROP TABLE IF EXISTS temp.execution_ids", [])?;

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn orphan_execution_ids(&self, executor_ids: &[Uuid]) -> eyre::Result<Vec<Uuid>> {
        let executor_ids: Vec<_> = executor_ids.into();

        self.with_db(move |db| {
            if executor_ids.is_empty() {
                let mut stmt = db.prepare(
                    r#"--sql
                        SELECT id
                        FROM ora_execution
                        WHERE
                            executor_id IS NOT NULL
                            AND (
                                succeeded_at_unix_ns IS NULL
                                AND failed_at_unix_ns IS NULL
                            );
                    "#,
                )?;

                let ids = stmt
                    .query_map(params_from_iter(&executor_ids), |row| row.get(0))?
                    .collect::<Result<Vec<Uuid>, _>>()?;

                Ok(ids)
            } else {
                let id_params_sql = "?,".repeat(executor_ids.len());
                let id_params_sql = &id_params_sql[..id_params_sql.len() - 1];

                let mut stmt = db.prepare(&format!(
                    r#"--sql
                        SELECT id
                        FROM ora_execution
                        WHERE
                            executor_id NOT IN ({id_params_sql})
                            AND (
                                succeeded_at_unix_ns IS NULL
                                AND failed_at_unix_ns IS NULL
                            );
                    "#
                ))?;

                let ids = stmt
                    .query_map(params_from_iter(&executor_ids), |row| row.get(0))?
                    .collect::<Result<Vec<Uuid>, _>>()?;

                Ok(ids)
            }
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn jobs_unschedulable(
        &self,
        job_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        const TEMP_TABLE_THRESHOLD: usize = DEFAULT_TEMP_TABLE_THRESHOLD;

        if job_ids.is_empty() {
            return Ok(());
        }

        let job_ids: Vec<_> = job_ids.into();

        self.with_db(move |db| {
            let tx = db.transaction()?;

            if job_ids.len() < TEMP_TABLE_THRESHOLD {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_job
                        SET marked_unschedulable_at_unix_ns = :marked_unschedulable_at_unix_ns
                        WHERE id = :id AND marked_unschedulable_at_unix_ns IS NULL;
                    "#,
                )?;

                for id in &job_ids {
                    update_stmt.execute(named_params![
                        ":marked_unschedulable_at_unix_ns": SqlSystemTime(timestamp),
                        ":id": id
                    ])?;
                }
            } else {
                {
                    tx.execute("CREATE TABLE temp.job_ids (id BLOB)", [])?;

                    let mut insert_stmt = tx.prepare("INSERT INTO temp.job_ids (id) VALUES (?)")?;

                    for id in &job_ids {
                        insert_stmt.execute(params![id])?;
                    }
                }

                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_job
                        SET marked_unschedulable_at_unix_ns = :marked_unschedulable_at_unix_ns
                        WHERE
                            id IN (SELECT id FROM temp.job_ids)
                            AND marked_unschedulable_at_unix_ns IS NULL;
                    "#,
                )?;

                update_stmt.execute(named_params![
                    ":marked_unschedulable_at_unix_ns": SqlSystemTime(timestamp)
                ])?;

                tx.execute("DROP TABLE temp.job_ids", [])?;
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn pending_executions(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::PendingExecution>> {
        self.with_db(move |db| {
            let mut stmt = db.prepare_cached(
                r#"--sql
                    SELECT
                        e.id,
                        j.target_execution_time_unix_ns
                    FROM ora_execution e
                    JOIN
                        ora_job j
                    ON
                        e.job_id = j.id
                    WHERE
                        e.ready_at_unix_ns IS NULL
                        AND (? IS NULL OR e.id > ?)
                    LIMIT 10000;
                "#,
            )?;

            let pending_executions = stmt
                .query_map(params![after, after], |row| {
                    Ok(ora_storage::PendingExecution {
                        id: row.get(0)?,
                        target_execution_time: row.get::<_, SqlSystemTime>(1)?.into(),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(pending_executions)
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn ready_executions(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::ReadyExecution>> {
        self.with_db(move |db| {
            let mut stmt = db.prepare_cached(
                r#"--sql
                    SELECT
                        e.id,
                        e.job_id,
                        j.input_payload_json,
                        es.execution_count AS attempt_number,
                        j.job_type_id,
                        j.target_execution_time_unix_ns,
                        j.timeout_policy
                    FROM ora_execution e
                    JOIN ora_job j ON
                        j.id = e.job_id
                    JOIN ora_job_execution_state es ON
                        j.id = es.job_id
                    WHERE
                        ready_at_unix_ns IS NOT NULL
                        AND assigned_at_unix_ns IS NULL
                        AND started_at_unix_ns IS NULL
                        AND failed_at_unix_ns IS NULL
                        AND succeeded_at_unix_ns IS NULL
                        AND (? IS NULL OR e.id > ?)
                    ORDER BY e.id ASC
                    LIMIT 10000;
                "#,
            )?;

            let mut rows = stmt.query([after, after])?;

            let mut ready_executions = Vec::new();

            while let Some(row) = rows.next()? {
                ready_executions.push(ora_storage::ReadyExecution {
                    id: row.get(0)?,
                    job_id: row.get(1)?,
                    input_payload_json: row.get(2)?,
                    attempt_number: row.get(3)?,
                    job_type_id: row.get(4)?,
                    target_execution_time: row.get::<_, SqlSystemTime>(5)?.into(),
                    timeout_policy: row.get::<_, JobTimeoutPolicy>(6)?.into(),
                });
            }

            Ok(ready_executions)
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn pending_jobs(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::PendingJob>> {
        self.with_db(move |db| {
            let mut stmt = db.prepare_cached(
                r#"--sql
                    SELECT
                        ora_job.id,
                        target_execution_time_unix_ns,
                        es.execution_count,
                        retry_policy,
                        timeout_policy
                    FROM ora_job
                    JOIN ora_job_execution_state es ON
                        ora_job.id = es.job_id
                    WHERE
                        marked_unschedulable_at_unix_ns IS NULL
                        AND es.active_execution_id IS NULL
                        AND (? IS NULL OR id > ?)
                    ORDER BY ora_job.id ASC
                    LIMIT 10000;
                "#,
            )?;

            let mut rows = stmt.query([after, after])?;

            let mut pending_jobs = Vec::new();

            while let Some(row) = rows.next()? {
                pending_jobs.push(ora_storage::PendingJob {
                    id: row.get(0)?,
                    target_execution_time: row.get::<_, SqlSystemTime>(1)?.0,
                    execution_count: row.get(2)?,
                    retry_policy: row.get::<_, JobRetryPolicy>(3)?.into(),
                    timeout_policy: row.get::<_, JobTimeoutPolicy>(4)?.into(),
                });
            }

            Ok(pending_jobs)
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn query_jobs(
        &self,
        cursor: Option<String>,
        limit: usize,
        order: ora_storage::JobQueryOrder,
        filters: ora_storage::JobQueryFilters,
    ) -> eyre::Result<ora_storage::JobQueryResult> {
        let cursor: Option<job_query::Cursor> = match cursor {
            Some(cursor) => serde_json::from_str(&cursor).wrap_err("invalid cursor")?,
            None => None,
        };

        self.with_db_concurrent(move |db| {
            let mut tx = db.transaction()?;
            let res = job_query::query_job_details(&mut tx, cursor, limit, order, filters);
            tx.commit()?;
            res
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn query_job_ids(
        &self,
        filters: ora_storage::JobQueryFilters,
    ) -> eyre::Result<Vec<Uuid>> {
        self.with_db_concurrent(|db| {
            let mut tx = db.transaction()?;
            let res = job_query::job_ids(&mut tx, filters);
            tx.commit()?;
            res
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn count_jobs(&self, filters: ora_storage::JobQueryFilters) -> eyre::Result<u64> {
        self.with_db_concurrent(|db| {
            let mut tx = db.transaction()?;
            let res = job_query::count_jobs(&mut tx, filters);
            tx.commit()?;
            res
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn query_job_types(&self) -> eyre::Result<Vec<ora_storage::JobType>> {
        self.with_db_concurrent(|db| {
            let mut stmt = db.prepare_cached(
                r#"--sql
                    SELECT
                        id,
                        name,
                        description,
                        input_schema_json,
                        output_schema_json
                    FROM ora_job_type;
                "#,
            )?;

            let job_types = stmt
                .query_map([], |row| {
                    Ok(ora_storage::JobType {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        input_schema_json: row.get(3)?,
                        output_schema_json: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(job_types)
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn delete_jobs(&self, filters: ora_storage::JobQueryFilters) -> eyre::Result<Vec<Uuid>> {
        self.with_db(|db| {
            let mut tx = db.transaction()?;
            let res = job_query::delete_jobs(&mut tx, filters);
            tx.commit()?;
            res
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn schedules_added(&self, schedules: Vec<ora_storage::NewSchedule>) -> eyre::Result<()> {
        if schedules.is_empty() {
            return Ok(());
        }

        self.with_db(|db| {
            let tx = db.transaction()?;

            {
                let mut insert_stmt = tx.prepare_cached(
                    r#"--sql
                        INSERT INTO ora_schedule (
                            id,
                            created_at_unix_ns,
                            job_type_id,
                            job_timing_policy,
                            job_creation_policy,
                            start_after_unix_ns,
                            end_before_unix_ns,
                            metadata_json
                        )
                        VALUES (
                            :id,
                            :created_at_unix_ns,
                            :job_type_id,
                            :job_timing_policy,
                            :job_creation_policy,
                            :start_after_unix_ns,
                            :end_before_unix_ns,
                            :metadata_json
                        );
                    "#,
                )?;

                for schedule in schedules {
                    let job_type_id = match &schedule.job_creation_policy {
                        ora_storage::ScheduleJobCreationPolicy::JobDefinition(
                            schedule_new_job_definition,
                        ) => schedule_new_job_definition.job_type_id.clone(),
                    };

                    let start_after = schedule.time_range.as_ref().and_then(|r| r.start);
                    let end_before = schedule.time_range.as_ref().and_then(|r| r.end);

                    insert_stmt.execute(params![
                        schedule.id,
                        SqlSystemTime(schedule.created_at),
                        job_type_id,
                        crate::models::ScheduleJobTimingPolicy::from(schedule.job_timing_policy),
                        crate::models::ScheduleJobCreationPolicy::from(
                            schedule.job_creation_policy
                        ),
                        start_after.map(SqlSystemTime),
                        end_before.map(SqlSystemTime),
                        schedule.metadata_json
                    ])?;

                    let mut insert_label_stmt = tx.prepare_cached(
                        r#"--sql
                            INSERT INTO ora_schedule_label (
                                schedule_id,
                                key,
                                value
                            )
                            VALUES (
                                :schedule_id,
                                :key,
                                :value
                            );
                        "#,
                    )?;

                    for (key, value) in schedule.labels {
                        insert_label_stmt.execute(params![schedule.id, key, value])?;
                    }
                }
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn schedules_cancelled(
        &self,
        schedule_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<Vec<ora_storage::CancelledSchedule>> {
        const TEMP_TABLE_THRESHOLD: usize = DEFAULT_TEMP_TABLE_THRESHOLD;

        if schedule_ids.is_empty() {
            return Ok(vec![]);
        }

        let schedule_ids: Vec<_> = schedule_ids.into();

        self.with_db(move |db| {
            let tx = db.transaction()?;

            let cancelled_schedules = if schedule_ids.len() < TEMP_TABLE_THRESHOLD {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_schedule
                        SET
                            cancelled_at_unix_ns = :cancelled_at_unix_ns,
                            marked_unschedulable_at_unix_ns = :cancelled_at_unix_ns
                        WHERE
                            id = :id
                            AND cancelled_at_unix_ns IS NULL
                            AND marked_unschedulable_at_unix_ns IS NULL
                        RETURNING id;
                    "#,
                )?;

                let mut cancelled_schedules = Vec::new();

                for id in &schedule_ids {
                    let mut rows = update_stmt.query_map(
                        named_params![
                            ":cancelled_at_unix_ns": SqlSystemTime(timestamp),
                            ":id": id
                        ],
                        |row| Ok(ora_storage::CancelledSchedule { id: row.get(0)? }),
                    )?;

                    if let Some(cancelled_schedule) = rows.next() {
                        let cancelled_schedule = cancelled_schedule?;
                        cancelled_schedules.push(cancelled_schedule);
                    }
                }

                cancelled_schedules
            } else {
                {
                    tx.execute("CREATE TABLE temp.schedule_ids (id BLOB)", [])?;

                    let mut insert_stmt =
                        tx.prepare("INSERT INTO temp.schedule_ids (id) VALUES (?)")?;

                    for id in &schedule_ids {
                        insert_stmt.execute(params![id])?;
                    }
                }

                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                    UPDATE ora_schedule
                    SET
                        cancelled_at_unix_ns = :cancelled_at_unix_ns,
                        marked_unschedulable_at_unix_ns = :cancelled_at_unix_ns
                    WHERE
                        id IN (SELECT id FROM temp.schedule_ids)
                        AND cancelled_at_unix_ns IS NULL
                        AND marked_unschedulable_at_unix_ns IS NULL
                    RETURNING id;
                    "#,
                )?;

                let cancelled_schedules = update_stmt
                    .query_map(
                        named_params![
                            ":cancelled_at_unix_ns": SqlSystemTime(timestamp)
                        ],
                        |row| Ok(ora_storage::CancelledSchedule { id: row.get(0)? }),
                    )?
                    .collect::<Result<Vec<_>, _>>()?;

                tx.execute("DROP TABLE temp.schedule_ids", [])?;

                cancelled_schedules
            };

            tx.commit()?;

            Ok(cancelled_schedules)
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn schedules_unschedulable(
        &self,
        schedule_ids: &[Uuid],
        timestamp: std::time::SystemTime,
    ) -> eyre::Result<()> {
        const TEMP_TABLE_THRESHOLD: usize = DEFAULT_TEMP_TABLE_THRESHOLD;

        if schedule_ids.is_empty() {
            return Ok(());
        }

        let schedule_ids: Vec<_> = schedule_ids.into();

        self.with_db(move |db| {
            let tx = db.transaction()?;

            if schedule_ids.len() < TEMP_TABLE_THRESHOLD {
                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_schedule
                        SET marked_unschedulable_at_unix_ns = :marked_unschedulable_at_unix_ns
                        WHERE id = :id AND marked_unschedulable_at_unix_ns IS NULL;
                    "#,
                )?;

                for id in &schedule_ids {
                    update_stmt.execute(named_params![
                        ":marked_unschedulable_at_unix_ns": SqlSystemTime(timestamp),
                        ":id": id
                    ])?;
                }
            } else {
                {
                    tx.execute("CREATE TABLE temp.schedule_ids (id BLOB)", [])?;

                    let mut insert_stmt =
                        tx.prepare("INSERT INTO temp.schedule_ids (id) VALUES (?)")?;

                    for id in &schedule_ids {
                        insert_stmt.execute(params![id])?;
                    }
                }

                let mut update_stmt = tx.prepare_cached(
                    r#"--sql
                        UPDATE ora_schedule
                        SET marked_unschedulable_at_unix_ns = :marked_unschedulable_at_unix_ns
                        WHERE
                            id IN (SELECT id FROM temp.schedule_ids)
                            AND marked_unschedulable_at_unix_ns IS NULL;
                    "#,
                )?;

                update_stmt.execute(named_params![
                    ":marked_unschedulable_at_unix_ns": SqlSystemTime(timestamp)
                ])?;

                tx.execute("DROP TABLE temp.schedule_ids", [])?;
            }

            tx.commit()?;

            Ok(())
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn pending_schedules(
        &self,
        after: Option<Uuid>,
    ) -> eyre::Result<Vec<ora_storage::PendingSchedule>> {
        self.with_db(move |db| {
            let mut stmt = db.prepare_cached(
                r#"--sql
                    SELECT
                        id,
                        job_timing_policy,
                        job_creation_policy,
                        last_target_execution_time_unix_ns,
                        start_after_unix_ns,
                        end_before_unix_ns
                    FROM ora_schedule_job_state
                    JOIN ora_schedule ON
                        ora_schedule_job_state.schedule_id = ora_schedule.id
                    WHERE
                        marked_unschedulable_at_unix_ns IS NULL
                        AND active_job_id IS NULL
                        AND (? IS NULL OR id > ?)
                    ORDER BY id ASC
                    LIMIT 10000;
                "#,
            )?;

            let mut rows = stmt.query([after, after])?;

            let mut pending_schedules = Vec::new();

            while let Some(row) = rows.next()? {
                pending_schedules.push(ora_storage::PendingSchedule {
                    id: row.get(0)?,
                    job_timing_policy: row
                        .get::<_, crate::models::ScheduleJobTimingPolicy>(1)?
                        .into(),
                    job_creation_policy: row
                        .get::<_, crate::models::ScheduleJobCreationPolicy>(2)?
                        .into(),
                    last_target_execution_time: row
                        .get::<_, Option<SqlSystemTime>>(3)?
                        .map(Into::into),
                    time_range: Some(ScheduleTimeRange {
                        start: row.get::<_, Option<SqlSystemTime>>(4)?.map(Into::into),
                        end: row.get::<_, Option<SqlSystemTime>>(5)?.map(Into::into),
                    }),
                });
            }

            Ok(pending_schedules)
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn query_schedules(
        &self,
        cursor: Option<String>,
        limit: usize,
        filters: ora_storage::ScheduleQueryFilters,
        order: ora_storage::ScheduleQueryOrder,
    ) -> eyre::Result<ora_storage::ScheduleQueryResult> {
        let cursor: Option<schedule_query::Cursor> = match cursor {
            Some(cursor) => serde_json::from_str(&cursor).wrap_err("invalid cursor")?,
            None => None,
        };

        self.with_db_concurrent(move |db| {
            let mut tx = db.transaction()?;
            let res =
                schedule_query::query_schedule_details(&mut tx, cursor, limit, order, filters);
            tx.commit()?;
            res
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn query_schedule_ids(
        &self,
        filters: ora_storage::ScheduleQueryFilters,
    ) -> eyre::Result<Vec<Uuid>> {
        self.with_db_concurrent(|db| {
            let mut tx = db.transaction()?;
            let res = schedule_query::schedule_ids(&mut tx, filters);
            tx.commit()?;
            res
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn count_schedules(
        &self,
        filters: ora_storage::ScheduleQueryFilters,
    ) -> eyre::Result<u64> {
        self.with_db_concurrent(|db| {
            let mut tx = db.transaction()?;
            let res = schedule_query::count_schedules(&mut tx, filters);
            tx.commit()?;
            res
        })
        .await
    }

    #[tracing::instrument(skip_all)]
    async fn delete_schedules(
        &self,
        filters: ora_storage::ScheduleQueryFilters,
    ) -> eyre::Result<Vec<Uuid>> {
        self.with_db(|db| {
            let mut tx = db.transaction()?;
            let res = schedule_query::delete_schedules(&mut tx, filters);
            tx.commit()?;
            res
        })
        .await
    }
}
