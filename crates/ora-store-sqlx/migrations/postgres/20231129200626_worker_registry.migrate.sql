CREATE TABLE IF NOT EXISTS "ora"."worker" (
    "id" UUID NOT NULL PRIMARY KEY,
    "created" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "last_seen" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "active" BOOLEAN NOT NULL DEFAULT TRUE,
    "name" TEXT,
    "description" TEXT,
    "version" TEXT,
    "supported_tasks" JSONB,
    "other_metadata" JSONB NOT NULL DEFAULT '{}'::JSONB
);
