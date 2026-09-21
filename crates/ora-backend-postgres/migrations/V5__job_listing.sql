-- Ordering a job type's jobs by target execution time had no index to
-- walk, so one page meant scanning and sorting every job of that type.
--
-- Building this locks out writes to ora.job until it finishes. On a
-- large table, create it first with CREATE INDEX CONCURRENTLY under
-- this name; the statement below then finds it and does nothing.
CREATE INDEX IF NOT EXISTS idx_job_type_target_time ON ora.job (job_type_id, target_execution_time, id);
