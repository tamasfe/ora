-- Filtering or counting finished jobs by status had to look up the latest
-- execution of every finished job, which does not finish in time with
-- millions of jobs. The status is now stored with the job when it finishes,
-- it is NULL for active jobs (like `inactive_since`).
--
-- This rewrites every finished job once, which can take a while on a large
-- table and locks out writes to ora.job until it finishes.
ALTER TABLE ora.job ADD COLUMN IF NOT EXISTS inactive_status SMALLINT;

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

-- Counting or listing (newest first) the jobs with a given status.
CREATE INDEX IF NOT EXISTS idx_job_status_id ON ora.job (inactive_status, id);

-- Counting jobs by job type and status, also replaces the index on the job type.
CREATE INDEX IF NOT EXISTS idx_job_type_status ON ora.job (job_type_id, inactive_status);
DROP INDEX IF EXISTS ora."ora_job_job_type_id_idx";

ANALYZE ora.job;
