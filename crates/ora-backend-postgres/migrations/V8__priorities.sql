-- Job priorities, existing jobs and executions get the default priority,
-- so no backfill is necessary (adding a column with a constant
-- default does not rewrite the table).
ALTER TABLE ora.job
    ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;

-- The priority of the job is copied to its executions
-- so that ready executions can be ordered without a join.
ALTER TABLE ora.execution
    ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;

-- Ready executions are ordered by priority descending and ID ascending,
-- the priority is negated (and cast to avoid overflows) so that
-- keyset pagination can use a single row comparison.
--
-- Building these locks out writes until they finish. On large
-- tables, create them first with CREATE INDEX CONCURRENTLY under
-- these names; the statements below then find them and do nothing.
CREATE INDEX IF NOT EXISTS idx_ready_executions_priority
ON ora.execution ((-priority::BIGINT), id) WHERE status = 0;

CREATE INDEX IF NOT EXISTS idx_job_priority_id ON ora.job (priority, id);
