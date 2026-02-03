CREATE SCHEMA IF NOT EXISTS ora;

CREATE TABLE IF NOT EXISTS ora.job_type (
    id TEXT NOT NULL PRIMARY KEY,
    description TEXT,
    input_schema_json TEXT,
    output_schema_json TEXT
);

CREATE TABLE IF NOT EXISTS ora.schedule (
    id UUID NOT NULL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    job_template_job_type_id TEXT NOT NULL, -- Stored separately for indexing
    job_template_json TEXT NOT NULL,
    scheduling_policy_json TEXT NOT NULL,
    start_after TIMESTAMPTZ,
    end_before TIMESTAMPTZ,
    stopped_at TIMESTAMPTZ,
    active_job_id UUID
);

CREATE INDEX IF NOT EXISTS "ora_schedule_job_type_id_idx" ON ora.schedule (job_template_job_type_id);

CREATE INDEX IF NOT EXISTS "ora_schedule_stopped_at_null_idx" ON ora.schedule (stopped_at);
CREATE INDEX IF NOT EXISTS "ora_schedule_pending_idx" ON ora.schedule (stopped_at, active_job_id);

CREATE TABLE IF NOT EXISTS ora.schedule_label (
    schedule_id UUID NOT NULL REFERENCES ora.schedule(id) ON DELETE CASCADE,
    label_key TEXT NOT NULL,
    label_value TEXT,
    PRIMARY KEY (schedule_id, label_key)
);

CREATE TABLE IF NOT EXISTS ora.job (
    id UUID NOT NULL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    schedule_id UUID REFERENCES ora.schedule(id) ON DELETE CASCADE,
    job_type_id TEXT NOT NULL,
    target_execution_time TIMESTAMPTZ NOT NULL,
    input_payload_json TEXT NOT NULL,
    timeout_policy_json TEXT NOT NULL,
    retry_policy_json TEXT NOT NULL,
    inactive BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS "ora_job_job_type_id_idx" ON ora.job (job_type_id);
CREATE INDEX IF NOT EXISTS "ora_job_schedule_idx" ON ora.job (schedule_id);
CREATE INDEX IF NOT EXISTS "ora_job_inactive_idx" ON ora.job (inactive) WHERE inactive;

CREATE TABLE IF NOT EXISTS ora.job_label (
    job_id UUID NOT NULL REFERENCES ora.job(id) ON DELETE CASCADE,
    label_key TEXT NOT NULL,
    label_value TEXT,
    PRIMARY KEY (job_id, label_key)
);

CREATE INDEX IF NOT EXISTS "ora_job_label_key_idx" ON ora.job_label (label_key);

CREATE TABLE IF NOT EXISTS ora.execution (
    id UUID NOT NULL PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES ora.job(id) ON DELETE CASCADE,
    executor_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    succeeded_at TIMESTAMPTZ,
    failed_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    output_json TEXT,
    failure_reason TEXT,
    status SMALLINT NOT NULL GENERATED ALWAYS AS (
        CASE
            WHEN succeeded_at IS NOT NULL THEN 2
            WHEN failed_at IS NOT NULL THEN 3
            WHEN cancelled_at IS NOT NULL THEN 4
            WHEN started_at IS NULL THEN 0
            ELSE 1
        END
    ) STORED
);

CREATE INDEX IF NOT EXISTS "ora_execution_job_id_idx" ON ora.execution (job_id);
CREATE INDEX IF NOT EXISTS "ora_execution_active_idx" ON ora.execution (job_id) WHERE status IN (0, 1);
