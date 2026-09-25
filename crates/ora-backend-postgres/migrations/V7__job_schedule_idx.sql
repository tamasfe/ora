-- Finding the latest job of a schedule (for pending schedules) or the jobs
-- of given schedules had no index to walk since the original index on
-- the schedule ID was replaced by one that starts with the job ID.
--
-- Building this locks out writes to ora.job until it finishes. On a
-- large table, create it first with CREATE INDEX CONCURRENTLY under
-- this name; the statement below then finds it and does nothing.
CREATE INDEX IF NOT EXISTS idx_job_schedule_id ON ora.job (schedule_id, id);
