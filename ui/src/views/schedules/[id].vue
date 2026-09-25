<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { create } from "@bufbuild/protobuf";
import { ScheduleFiltersSchema, ScheduleStatus } from "../../api/ora/admin/v1/schedules_pb";
import type { Label } from "../../api/ora/common/v1/label_pb";
import type { StopSchedulesRequest } from "../../components/StopSchedulesDialog.vue";
import { useOraAdminClient } from "../../grpc";
import { useLoader } from "../../util";
import { formatDuration, formatRelative, formatTimestamp, prettyJson } from "../../util/format";
import { backoffStrategyLabel, hasTimeout, timeoutBaseTimeLabel } from "../../util/job";
import { labelsToFilters } from "../../util/labels";
import { usePolling } from "../../util/polling";
import { jobsLink, schedulesLink, useRefreshInterval } from "../../util/route";
import { missedTimePolicyLabel } from "../../util/schedule";
import { scheduleStatusInfo } from "../../util/status";

const route = useRoute("/schedules/[id]");
const router = useRouter();
const client = useOraAdminClient();

// Reading the route in the loader would reload on any query change.
const id = computed(() => route.params.id);
const refresh = useRefreshInterval();

const loader = useLoader(
  () => id.value,
  async (scheduleId, signal) => {
    const res = await client.listSchedules(
      { filters: { scheduleIds: [scheduleId] }, pagination: { pageSize: 1 } },
      { signal },
    );
    return res.schedules[0] ?? null;
  },
);

usePolling(loader, refresh);

/** The schedule, not shown while another schedule is loading. */
const schedule = computed(() =>
  loader.stale.value ? undefined : (loader.data.value ?? undefined),
);
const notFound = computed(() => !loader.stale.value && loader.data.value === null);
const definition = computed(() => schedule.value?.schedule);
const template = computed(() => definition.value?.jobTemplate);
const status = computed(() =>
  schedule.value ? scheduleStatusInfo[schedule.value.status] : undefined,
);
const policy = computed(() => definition.value?.scheduling?.policy);
const jobsFilters = computed(() => ({ scheduleIds: [id.value] }));

const jobsTable = ref<{ reload(force?: boolean): void }>();
const stopRequest = ref<StopSchedulesRequest>();

function stop() {
  stopRequest.value = {
    filters: create(ScheduleFiltersSchema, { scheduleIds: [id.value] }),
    message: "Stop this schedule? No new jobs will be created.",
  };
}

function onStopped() {
  loader.reload(true);
  jobsTable.value?.reload(true);
}

function reload() {
  loader.reload();
  jobsTable.value?.reload();
}

function scheduleLabelLink(label: Label) {
  return schedulesLink({ labels: [{ key: label.key, value: label.value }] });
}

function jobLabelLink(label: Label) {
  return jobsLink({ labels: [{ key: label.key, value: label.value }] });
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
        <h1 class="text-2xl font-semibold">Schedule</h1>
        <CopyableId :id="id" class="text-muted-color" />
        <Tag v-if="status" :value="status.label" :severity="status.severity" :icon="status.icon" />
        <Skeleton v-else-if="!notFound" width="6rem" height="1.75rem" />
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <LoadStatus :state="loader" verb="load" updated class="min-w-56 text-right" />
        <RefreshControl v-model="refresh" :loading="loader.busy.value" @refresh="reload" />
        <RouterLink
          v-slot="{ navigate }"
          :to="{ path: '/schedules/new', query: { from: id } }"
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
          label="Stop"
          icon="pi pi-stop-circle"
          severity="danger"
          :disabled="schedule?.status !== ScheduleStatus.ACTIVE"
          @click="stop"
        />
      </div>
    </div>

    <Message v-if="notFound" severity="warn">Schedule not found.</Message>

    <template v-else>
      <div class="flex min-h-7 flex-wrap items-center gap-2">
        <span class="text-sm text-muted-color"><i class="pi pi-tags mr-1" />Labels</span>
        <template v-if="definition">
          <LabelList
            v-if="definition.labels.length > 0"
            :labels="definition.labels"
            size="normal"
            :link="scheduleLabelLink"
            hint="Show schedules with this label"
          />
          <span v-else class="text-sm text-muted-color">None</span>
          <RouterLink
            v-if="definition.labels.length > 1"
            :to="schedulesLink({ labels: labelsToFilters(definition.labels) })"
            class="text-sm text-primary hover:underline"
          >
            Schedules with the same labels
          </RouterLink>
        </template>
        <Skeleton v-else width="16rem" height="1.5rem" />
      </div>

      <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card>
          <template #title>Scheduling</template>
          <template #content>
            <dl
              v-if="schedule && definition"
              class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm"
            >
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
            </dl>
            <div v-else class="flex flex-col gap-3">
              <Skeleton v-for="i in 6" :key="i" height="1.25rem" />
            </div>
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
                <dt class="text-muted-color">Labels</dt>
                <dd class="min-w-0">
                  <LabelList
                    v-if="template.labels.length > 0"
                    :labels="template.labels"
                    :link="jobLabelLink"
                    hint="Show jobs with this label"
                  />
                  <span v-else>-</span>
                </dd>
                <dt class="text-muted-color">Priority</dt>
                <dd class="tabular-nums">{{ template.priority }}</dd>
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
              </dl>
              <JsonEditor :model-value="prettyJson(template.inputPayloadJson)" readonly />
            </div>
            <div v-else-if="!schedule" class="flex flex-col gap-3">
              <Skeleton v-for="i in 4" :key="i" height="1.25rem" />
              <Skeleton height="6rem" />
            </div>
          </template>
        </Card>
      </div>

      <div class="flex flex-col gap-2">
        <h2 class="text-xl font-semibold">Jobs</h2>
        <!-- Only depends on the ID, so it is not held back by loading the schedule. -->
        <JobsTable
          ref="jobsTable"
          :base-filters="jobsFilters"
          query-prefix="jobs."
          :refresh-interval="refresh"
        />
      </div>
    </template>

    <StopSchedulesDialog v-model="stopRequest" @stopped="onStopped" />
  </div>
</template>
