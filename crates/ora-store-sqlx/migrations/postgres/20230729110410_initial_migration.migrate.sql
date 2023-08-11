CREATE SCHEMA IF NOT EXISTS "ora";

CREATE FUNCTION "ora"."update_updated_column"() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  NEW."updated" = NOW();
  RETURN NEW;
END;
$$;

CREATE FUNCTION "ora"."notify_task_added"() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  PERFORM pg_notify('ora_task_added', NEW."id"::TEXT);
  RETURN NULL;
END;
$$;

CREATE FUNCTION "ora"."notify_schedule_added"() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  PERFORM pg_notify('ora_schedule_added', NEW."id"::TEXT);
  RETURN NULL;
END;
$$;

CREATE TABLE IF NOT EXISTS "ora"."schedule" (
    "id" UUID NOT NULL PRIMARY KEY,
    "updated" TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    "active" BOOLEAN NOT NULL GENERATED ALWAYS AS ("cancelled_at" IS NULL) STORED,

    "schedule_policy" JSONB NOT NULL,
    "immediate" BOOLEAN NOT NULL,
    "missed_tasks_policy" JSONB NOT NULL,
    "new_task" JSONB NOT NULL,
    "labels" JSONB NOT NULL DEFAULT '{}',

    "added_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "cancelled_at" TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS "task_labels" ON "ora"."schedule" USING GIN ("labels");
CREATE INDEX IF NOT EXISTS "task_status" ON "ora"."schedule" ("active") WHERE "active";

CREATE OR REPLACE TRIGGER "schedule_updated"
BEFORE UPDATE ON "ora"."schedule"
FOR EACH ROW EXECUTE FUNCTION "ora"."update_updated_column"();

CREATE OR REPLACE TRIGGER "notify_schedule_added"
AFTER INSERT ON "ora"."schedule"
FOR EACH ROW EXECUTE FUNCTION "ora"."notify_schedule_added"();

CREATE TABLE IF NOT EXISTS "ora"."task" (
    "id" UUID NOT NULL PRIMARY KEY,
    "updated" TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    "status" TEXT NOT NULL CHECK ("status" IN (
        'pending',
        'ready',
        'started',
        'succeeded',
        'failed',
        'cancelled'
    )),
    "active" BOOLEAN NOT NULL GENERATED ALWAYS AS ("status" NOT IN ('succeeded', 'failed', 'cancelled')) STORED,

    "schedule_id" UUID REFERENCES "ora"."schedule"("id"),

    "target" TIMESTAMPTZ NOT NULL,
    "worker_selector" JSONB NOT NULL,
    "data_bytes" BYTEA,
    "data_json" JSONB,
    "data_format" TEXT NOT NULL CHECK ("data_format" IN ('unknown', 'message_pack', 'json')),
    "labels" JSONB NOT NULL DEFAULT '{}',
    "timeout_policy" JSONB NOT NULL,

    "added_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "ready_at" TIMESTAMPTZ,
    "started_at" TIMESTAMPTZ,
    "succeeded_at" TIMESTAMPTZ,
    "failed_at" TIMESTAMPTZ,
    "cancelled_at" TIMESTAMPTZ,

    "output_bytes" BYTEA,
    "output_json" JSONB,
    "output_format" TEXT CHECK ("data_format" IN ('unknown', 'message_pack', 'json')),
    "failure_reason" TEXT
);

CREATE INDEX IF NOT EXISTS "worker_selectors" ON "ora"."task" USING GIN ("worker_selector");
CREATE INDEX IF NOT EXISTS "task_labels" ON "ora"."task" USING GIN ("labels");
CREATE INDEX IF NOT EXISTS "task_status" ON "ora"."task" ("active") WHERE "active";
CREATE INDEX IF NOT EXISTS "schedule_id" ON "ora"."task" ("schedule_id");

CREATE OR REPLACE TRIGGER "task_updated"
BEFORE UPDATE ON "ora"."task"
FOR EACH ROW EXECUTE FUNCTION "ora"."update_updated_column"();

CREATE OR REPLACE TRIGGER "notify_task_added"
AFTER INSERT ON "ora"."task"
FOR EACH ROW EXECUTE FUNCTION "ora"."notify_task_added"();
