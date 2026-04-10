LOCK TABLE ora.execution IN EXCLUSIVE MODE;

ALTER TABLE ora.execution
    ADD COLUMN target_execution_time timestamptz;

UPDATE
    ora.execution
SET
    target_execution_time = ora.job.target_execution_time
FROM
    ora.job
WHERE
    ora.execution.job_id = ora.job.id;

ALTER TABLE ora.execution
    ALTER COLUMN target_execution_time SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_ready_executions
ON ora.execution (target_execution_time ASC, id, job_id) WHERE status = 0;
