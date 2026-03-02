ALTER TABLE ora.job DROP CONSTRAINT IF EXISTS job_schedule_id_fkey;

CREATE INDEX IF NOT EXISTS idx_job_inactive_since_id
ON ora.job (inactive_since, id);
