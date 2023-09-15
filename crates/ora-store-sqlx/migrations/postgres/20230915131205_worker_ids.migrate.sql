ALTER TABLE
    "ora"."task"
ADD
    COLUMN IF NOT EXISTS "worker_id" UUID;