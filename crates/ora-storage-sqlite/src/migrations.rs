use rusqlite::Connection;

pub(super) fn run_migrations(db: &mut Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        r#"--sql
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

        -- FIXME(perf): add indexes
        "#,
    )?;

    Ok(())
}
