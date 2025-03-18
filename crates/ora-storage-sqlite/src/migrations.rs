use rusqlite::{Connection, OptionalExtension};

pub(super) fn run_migrations(db: &mut Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS ora_migrations (ver INTEGER PRIMARY KEY NOT NULL);",
    )?;

    let current_version: i64 = db
        .query_row(
            "SELECT ver FROM ora_migrations ORDER BY ver DESC LIMIT 1;",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);

    if current_version < 1 {
        db.execute_batch(
            r#"--sql
            BEGIN;
            CREATE TABLE IF NOT EXISTS ora_job_type (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT,
                description TEXT,
                input_schema_json TEXT,
                output_schema_json TEXT
            );

            CREATE TABLE IF NOT EXISTS ora_job (
                id BLOB PRIMARY KEY NOT NULL,
                schedule_id BLOB,
                created_at_unix_ns INTEGER NOT NULL,
                job_type_id TEXT NOT NULL,
                target_execution_time_unix_ns INTEGER NOT NULL,
                retry_policy BLOB NOT NULL,
                timeout_policy BLOB NOT NULL,
                input_payload_json TEXT NOT NULL,
                metadata_json TEXT,
                cancelled_at_unix_ns INTEGER,
                marked_unschedulable_at_unix_ns INTEGER
            );

            CREATE TABLE IF NOT EXISTS ora_job_label (
                job_id BLOB NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                PRIMARY KEY (job_id, key)
            );

            CREATE INDEX IF NOT EXISTS ora_job_schedule_id ON ora_job (schedule_id);
            CREATE INDEX IF NOT EXISTS ora_job_target_execution_time_unix_ns ON ora_job (target_execution_time_unix_ns);
            CREATE INDEX IF NOT EXISTS ora_job_job_type_id ON ora_job (job_type_id);
            CREATE INDEX IF NOT EXISTS ora_job_marked_unschedulable_at_unix_ns ON ora_job (marked_unschedulable_at_unix_ns) WHERE marked_unschedulable_at_unix_ns IS NULL;

            CREATE TABLE IF NOT EXISTS ora_execution (
                id BLOB PRIMARY KEY NOT NULL,
                job_id BLOB NOT NULL,
                executor_id BLOB,
                created_at_unix_ns INTEGER NOT NULL,
                ready_at_unix_ns INTEGER,
                assigned_at_unix_ns INTEGER,
                started_at_unix_ns INTEGER,
                succeeded_at_unix_ns INTEGER,
                failed_at_unix_ns INTEGER,
                output_payload_json TEXT,
                failure_reason TEXT,
                active BOOLEAN NOT NULL GENERATED ALWAYS AS (
                    failed_at_unix_ns IS NULL
                    AND succeeded_at_unix_ns IS NULL
                ) STORED,
                "status" INTEGER NOT NULL GENERATED ALWAYS AS (
                    CASE
                        WHEN failed_at_unix_ns IS NOT NULL THEN 5
                        WHEN succeeded_at_unix_ns IS NOT NULL THEN 4
                        WHEN started_at_unix_ns IS NOT NULL THEN 3
                        WHEN assigned_at_unix_ns IS NOT NULL THEN 2
                        WHEN ready_at_unix_ns IS NOT NULL THEN 1
                        ELSE 0
                    END
                ) STORED
            );

            CREATE INDEX IF NOT EXISTS ora_execution_job_id ON ora_execution (job_id);
            CREATE INDEX IF NOT EXISTS ora_execution_executor_id ON ora_execution (executor_id);
            CREATE INDEX IF NOT EXISTS ora_execution_status ON ora_execution ("status");
            CREATE INDEX IF NOT EXISTS ora_execution_active ON ora_execution (active) WHERE active;

            CREATE TABLE IF NOT EXISTS ora_schedule (
                id BLOB PRIMARY KEY NOT NULL,
                created_at_unix_ns INTEGER NOT NULL,
                job_type_id TEXT,
                marked_unschedulable_at_unix_ns INTEGER,
                cancelled_at_unix_ns INTEGER,
                job_timing_policy BLOB NOT NULL,
                job_creation_policy BLOB NOT NULL,
                start_after_unix_ns INTEGER,
                end_before_unix_ns INTEGER,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS ora_schedule_label (
                schedule_id BLOB NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                PRIMARY KEY (schedule_id, key)
            );

            CREATE INDEX IF NOT EXISTS ora_schedule_job_type_id ON ora_schedule (job_type_id);
            CREATE INDEX IF NOT EXISTS ora_schedule_marked_unschedulable_at_unix_ns ON ora_schedule (marked_unschedulable_at_unix_ns) WHERE marked_unschedulable_at_unix_ns IS NULL;

            CREATE INDEX IF NOT EXISTS ora_execution_ready_at_unix_ns ON ora_execution (ready_at_unix_ns) WHERE ready_at_unix_ns IS NULL;
            CREATE INDEX IF NOT EXISTS ora_job_created_at_unix_ns ON ora_job (created_at_unix_ns);
            CREATE INDEX IF NOT EXISTS ora_execution_running_executor ON ora_execution (executor_id) WHERE succeeded_at_unix_ns IS NULL AND failed_at_unix_ns IS NULL;
            CREATE INDEX IF NOT EXISTS ora_schedule_created_at_unix_ns ON ora_schedule (created_at_unix_ns);

            INSERT INTO ora_migrations (ver) VALUES (1);
            COMMIT;
            "#,
        )?;
    }

    if current_version < 2 {
        db.execute_batch(
            r#"--sql
            BEGIN;
            CREATE TABLE IF NOT EXISTS ora_schedule_job_state (
                schedule_id BLOB PRIMARY KEY NOT NULL,
                active_job_id BLOB,
                last_target_execution_time_unix_ns INTEGER
            );

            CREATE TRIGGER IF NOT EXISTS create_schedule_job_state
            AFTER INSERT ON ora_schedule
            FOR EACH ROW
            BEGIN
                INSERT INTO ora_schedule_job_state (
                    schedule_id
                ) VALUES (
                    NEW.id
                );
            END;

            CREATE TRIGGER IF NOT EXISTS update_schedule_active_job
            AFTER INSERT ON ora_job
            FOR EACH ROW
            WHEN
                NEW.schedule_id IS NOT NULL
            BEGIN
                INSERT INTO ora_schedule_job_state (
                    schedule_id,
                    active_job_id,
                    last_target_execution_time_unix_ns
                ) VALUES (
                    NEW.schedule_id,
                    NEW.id,
                    NEW.target_execution_time_unix_ns
                )
                ON CONFLICT (schedule_id) DO UPDATE
                SET
                    active_job_id = NEW.id,
                    last_target_execution_time_unix_ns = NEW.target_execution_time_unix_ns;
            END;

            CREATE TRIGGER IF NOT EXISTS remove_schedule_active_job
            AFTER UPDATE ON ora_job
            FOR EACH ROW
            WHEN
                NEW.schedule_id IS NOT NULL
                AND NEW.marked_unschedulable_at_unix_ns IS NOT NULL
            BEGIN
                UPDATE ora_schedule_job_state
                SET
                    active_job_id = NULL
                WHERE schedule_id = NEW.schedule_id;
            END;

            INSERT INTO ora_schedule_job_state (
                schedule_id,
                active_job_id,
                last_target_execution_time_unix_ns
            ) SELECT
                id AS schedule_id,
                (
                    SELECT
                        id
                    FROM
                        ora_job
                    WHERE
                        schedule_id = ora_schedule.id
                        AND marked_unschedulable_at_unix_ns IS NULL
                ) AS active_job_id,
                (
                    SELECT
                        MAX(target_execution_time_unix_ns)
                    FROM
                        ora_job
                    WHERE
                        schedule_id = ora_schedule.id
                ) AS last_target_execution_time_unix_ns
            FROM ora_schedule;

            CREATE INDEX ora_schedule_no_active_job ON ora_schedule_job_state (schedule_id) WHERE active_job_id IS NULL;

            INSERT INTO ora_migrations (ver) VALUES (2);
            COMMIT;

            ANALYZE;
            "#,
        )?;
    }

    db.execute("PRAGMA optimize=0x10002", [])?;

    Ok(())
}
