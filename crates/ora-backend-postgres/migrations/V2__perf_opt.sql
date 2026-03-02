DROP INDEX IF EXISTS ora."ora_schedule_pending_idx";
DROP INDEX IF EXISTS ora."ora_job_schedule_idx";
DROP INDEX IF EXISTS ora."ora_job_label_key_idx";

CREATE INDEX IF NOT EXISTS "ora_schedule_pending_idx" ON ora.schedule (id, stopped_at, active_job_id)
WHERE
    stopped_at IS NULL
    AND active_job_id IS NULL;

CREATE INDEX IF NOT EXISTS "ora_job_schedule_idx" ON ora.job (id, schedule_id);

CREATE INDEX IF NOT EXISTS "ora_job_label_search_idx" ON ora.job_label (label_key, label_value, job_id);
CREATE INDEX IF NOT EXISTS "ora_schedule_label_search_idx" ON ora.schedule_label (label_key, label_value, schedule_id);

ALTER TABLE ora.job ADD COLUMN inactive_since TIMESTAMPTZ;

UPDATE ora.job
SET inactive_since = e.last_status_time
FROM
(
    SELECT
        COALESCE(e.succeeded_at, e.failed_at, e.cancelled_at) AS last_status_time, ora.job.id
    FROM
        ora.job
    JOIN LATERAL (
        SELECT
            status,
            succeeded_at,
            failed_at,
            cancelled_at
        FROM
            ora.execution
        WHERE
            ora.execution.job_id = ora.job.id
        ORDER BY ora.execution.id DESC
        LIMIT 1
    ) e ON TRUE
    WHERE
        ora.job.inactive
) e
WHERE ora.job.id = e.id;

ALTER TABLE ora.job DROP COLUMN inactive;

ANALYZE ora.schedule;
ANALYZE ora.job;
ANALYZE ora.schedule_label;
ANALYZE ora.job_label;
