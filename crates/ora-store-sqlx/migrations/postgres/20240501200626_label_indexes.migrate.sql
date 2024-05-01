CREATE INDEX IF NOT EXISTS "task_labels_idx" ON "ora"."task" USING gin ("labels");
CREATE INDEX IF NOT EXISTS "schedule_labels_idx" ON "ora"."schedule" USING gin ("labels");
