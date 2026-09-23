<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { create } from "@bufbuild/protobuf";
import { ScheduleFiltersSchema, ScheduleStatus } from "../../api/ora/admin/v1/schedules_pb";
import type { StopSchedulesRequest } from "../../components/StopSchedulesDialog.vue";
import { useOraAdminClient } from "../../grpc";
import { useLoader } from "../../util";
import { formatDuration, formatRelative, formatTimestamp, prettyJson } from "../../util/format";
import { backoffStrategyLabel, hasTimeout, timeoutBaseTimeLabel } from "../../util/job";
import { missedTimePolicyLabel } from "../../util/schedule";
import { scheduleStatusInfo } from "../../util/status";

const route = useRoute("/schedules/[id]");
const router = useRouter();
const client = useOraAdminClient();

const loader = useLoader(async signal => {
  const res = await client.listSchedules(
    { filters: { scheduleIds: [route.params.id] }, pagination: { pageSize: 1 } },
    { signal },
  );
  return res.schedules[0] ?? null;
});

const schedule = computed(() => loader.data.value);
const definition = computed(() => schedule.value?.schedule);
const template = computed(() => definition.value?.jobTemplate);
const status = computed(() =>
  schedule.value ? scheduleStatusInfo[schedule.value.status] : undefined,
);
const policy = computed(() => definition.value?.scheduling?.policy);

const jobsTable = ref<{ reload(): void }>();
const stopRequest = ref<StopSchedulesRequest>();

function stop() {
  stopRequest.value = {
    filters: create(ScheduleFiltersSchema, { scheduleIds: [route.params.id] }),
    message: "Stop this schedule? No new jobs will be created.",
  };
}

function onStopped() {
  loader.reload();
  jobsTable.value?.reload();
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
        <h1 class="text-2xl font-semibold">Schedule</h1>
        <CopyableId :id="route.params.id" class="text-muted-color" />
        <Tag v-if="status" :value="status.label" :severity="status.severity" :icon="status.icon" />
      </div>
      <div class="flex items-center gap-2">
        <RefreshControl :loading="loader.loading.value" @refresh="loader.reload" />
        <RouterLink
          v-slot="{ navigate }"
          :to="{ path: '/schedules/new', query: { from: route.params.id } }"
          custom
        >
          <Button
            label="Clone"
            icon="pi pi-clone"
            severity="secondary"
            outlined
            :disabled="!schedule"
            @click="navigate"
          />
        </RouterLink>
        <Button
          v-if="schedule?.status === ScheduleStatus.ACTIVE"
          label="Stop"
          icon="pi pi-stop-circle"
          severity="danger"
          @click="stop"
        />
      </div>
    </div>

    <Message v-if="loader.data.value === null" severity="warn">Schedule not found.</Message>
    <ProgressBar v-else-if="!schedule && loader.loading.value" mode="indeterminate" class="h-1!" />

    <template v-if="schedule && definition">
      <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card>
          <template #title>Scheduling</template>
          <template #content>
            <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
              <template v-if="policy?.case === 'interval'">
                <dt class="text-muted-color">Interval</dt>
                <dd>Every {{ formatDuration(policy.value.interval) }}</dd>
              </template>
              <template v-else-if="policy?.case === 'cron'">
                <dt class="text-muted-color">Cron expression</dt>
                <dd class="font-mono">{{ policy.value.cronExpression }}</dd>
              </template>
              <template v-if="policy?.case">
                <dt class="text-muted-color">Immediate</dt>
                <dd>{{ policy.value.immediate ? "Yes" : "No" }}</dd>
                <dt class="text-muted-color">Missed times</dt>
                <dd>{{ missedTimePolicyLabel(policy.value.missedTimePolicy) }}</dd>
              </template>
              <dt class="text-muted-color">Active from</dt>
              <dd>
                {{
                  definition.timeRange?.start
                    ? formatTimestamp(definition.timeRange.start)
                    : "Creation"
                }}
              </dd>
              <dt class="text-muted-color">Active until</dt>
              <dd>
                {{
                  definition.timeRange?.end
                    ? formatTimestamp(definition.timeRange.end)
                    : "Indefinitely"
                }}
              </dd>
              <dt class="text-muted-color">Created</dt>
              <dd>
                {{ formatTimestamp(schedule.createdAt) }}
                <span class="text-muted-color">({{ formatRelative(schedule.createdAt) }})</span>
              </dd>
              <template v-if="schedule.stoppedAt">
                <dt class="text-muted-color">Stopped</dt>
                <dd>
                  {{ formatTimestamp(schedule.stoppedAt) }}
                  <span class="text-muted-color">({{ formatRelative(schedule.stoppedAt) }})</span>
                </dd>
              </template>
              <dt class="text-muted-color">Labels</dt>
              <dd class="flex flex-wrap gap-1">
                <Tag
                  v-for="label in definition.labels"
                  :key="label.key"
                  :value="`${label.key}=${label.value}`"
                  severity="secondary"
                  class="font-normal!"
                />
                <span v-if="definition.labels.length === 0">-</span>
              </dd>
            </dl>
          </template>
        </Card>

        <Card>
          <template #title>Job template</template>
          <template #content>
            <div v-if="template" class="flex flex-col gap-3">
              <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
                <dt class="text-muted-color">Job type</dt>
                <dd>
                  <RouterLink
                    :to="`/job-types/${template.jobTypeId}`"
                    class="font-mono text-primary hover:underline"
                  >
                    {{ template.jobTypeId }}
                  </RouterLink>
                </dd>
                <dt class="text-muted-color">Timeout</dt>
                <dd>
                  <template v-if="hasTimeout(template.timeoutPolicy)">
                    {{ formatDuration(template.timeoutPolicy?.timeout) }}
                    <span class="text-muted-color">
                      (from
                      {{ timeoutBaseTimeLabel(template.timeoutPolicy!.baseTime).toLowerCase() }})
                    </span>
                  </template>
                  <span v-else>None</span>
                </dd>
                <dt class="text-muted-color">Retries</dt>
                <dd>
                  {{ template.retryPolicy?.retries ?? 0 }}
                  <span
                    v-if="template.retryPolicy && template.retryPolicy.retries > 0n"
                    class="text-muted-color"
                  >
                    ({{
                      backoffStrategyLabel(template.retryPolicy.backoffStrategy).toLowerCase()
                    }}
                    backoff {{ formatDuration(template.retryPolicy.backoffDuration) }})
                  </span>
                </dd>
                <dt class="text-muted-color">Labels</dt>
                <dd class="flex flex-wrap gap-1">
                  <Tag
                    v-for="label in template.labels"
                    :key="label.key"
                    :value="`${label.key}=${label.value}`"
                    severity="secondary"
                    class="font-normal!"
                  />
                  <span v-if="template.labels.length === 0">-</span>
                </dd>
              </dl>
              <JsonEditor :model-value="prettyJson(template.inputPayloadJson)" readonly />
            </div>
          </template>
        </Card>
      </div>

      <div class="flex flex-col gap-2">
        <h2 class="text-xl font-semibold">Jobs</h2>
        <JobsTable ref="jobsTable" :base-filters="{ scheduleIds: [route.params.id] }" />
      </div>
    </template>

    <StopSchedulesDialog v-model="stopRequest" @stopped="onStopped" />
  </div>
</template>
