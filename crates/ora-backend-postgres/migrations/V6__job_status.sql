-- Filtering or counting finished jobs by status had to look up the latest
-- execution of every finished job, which does not finish in time with
-- millions of jobs. The status is now stored with the job when it finishes,
-- it is NULL for active jobs (like `inactive_since`).
--
-- This rewrites every finished job once, which can take a while on a large
-- table and locks out ora.job until it finishes.
ALTER TABLE ora.job ADD COLUMN IF NOT EXISTS inactive_status SMALLINT;

-- Updating the indexes for every rewritten job takes most of the time,
-- so the indexes that can be are dropped and built again afterwards
-- (the index on the job type is replaced below).
DROP INDEX IF EXISTS ora."ora_job_job_type_id_idx";
DROP INDEX IF EXISTS ora."ora_job_schedule_idx";
DROP INDEX IF EXISTS ora.idx_job_inactive_since_id;
DROP INDEX IF EXISTS ora.idx_job_type_target_time;

UPDATE ora.job
SET inactive_status = e.status
FROM (
    SELECT DISTINCT ON (job_id)
        job_id,
        status
    FROM ora.execution
    ORDER BY job_id, id DESC
) e
WHERE
    ora.job.id = e.job_id
    AND ora.job.inactive_since IS NOT NULL;

-- The same indexes as before (see V2, V3 and V5).
CREATE INDEX IF NOT EXISTS "ora_job_schedule_idx" ON ora.job (id, schedule_id);
CREATE INDEX IF NOT EXISTS idx_job_inactive_since_id ON ora.job (inactive_since, id);
CREATE INDEX IF NOT EXISTS idx_job_type_target_time ON ora.job (job_type_id, target_execution_time, id);

-- Counting or listing (newest first) the jobs with a given status.
CREATE INDEX IF NOT EXISTS idx_job_status_id ON ora.job (inactive_status, id);

-- Counting jobs by job type and status, also replaces the index on the job type.
CREATE INDEX IF NOT EXISTS idx_job_type_status ON ora.job (job_type_id, inactive_status);

ANALYZE ora.job;
