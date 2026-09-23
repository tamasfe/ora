<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { create, type MessageInitShape } from "@bufbuild/protobuf";
import {
  ScheduleFiltersSchema,
  ScheduleOrderBy,
  ScheduleStatus,
  type Schedule,
  type ScheduleFilters,
} from "../api/ora/admin/v1/schedules_pb";
import { useOraAdminClient } from "../grpc";
import { useLoader } from "../util";
import { formatRelative, formatTimestamp } from "../util/format";
import { useTokenPagination } from "../util/pagination";
import { schedulingSummary } from "../util/schedule";
import type { StopSchedulesRequest } from "./StopSchedulesDialog.vue";
import { scheduleStatusInfo } from "../util/status";

const props = withDefaults(
  defineProps<{
    /** Fixed filters that are always applied and cannot be changed. */
    baseFilters?: MessageInitShape<typeof ScheduleFiltersSchema>;
    /** Hide the filters and bulk actions. */
    compact?: boolean;
    rows?: number;
  }>(),
  { rows: 20 },
);

const filters = defineModel<ScheduleFilters>("filters", {
  default: () => create(ScheduleFiltersSchema),
});

const client = useOraAdminClient();

const orderBy = ref(ScheduleOrderBy.CREATED_AT_DESC);
const orderOptions = [
  { label: "Newest first", value: ScheduleOrderBy.CREATED_AT_DESC },
  { label: "Oldest first", value: ScheduleOrderBy.CREATED_AT_ASC },
];

// Parents often pass inline objects, compare by value to avoid needless reloads.
const baseFiltersJson = computed(() => JSON.stringify(props.baseFilters ?? {}));
const baseFilters = computed(() =>
  create(ScheduleFiltersSchema, JSON.parse(baseFiltersJson.value)),
);
const baseKeys = computed(
  () => Object.keys(JSON.parse(baseFiltersJson.value)) as (keyof ScheduleFilters)[],
);

/** The filters sent to the server. */
const effectiveFilters = computed<ScheduleFilters>(() => {
  const result = { ...filters.value };
  for (const key of baseKeys.value) {
    (result as any)[key] = baseFilters.value[key];
  }
  return result;
});

const pagination = useTokenPagination(props.rows);

watch([effectiveFilters, orderBy], () => pagination.reset());

const list = useLoader(async signal => {
  const requestFilters = effectiveFilters.value;
  const listRequest = client.listSchedules(
    {
      filters: requestFilters,
      orderBy: orderBy.value,
      pagination: {
        pageSize: pagination.rows.value,
        nextPageToken: pagination.pageToken.value,
      },
    },
    { signal },
  );
  const countRequest = client.countSchedules({ filters: requestFilters }, { signal });

  const [res, count] = await Promise.all([listRequest, countRequest]);
  pagination.update(res.schedules.length, res.nextPageToken, Number(count.count));

  return { schedules: res.schedules, count: Number(count.count) };
});

const schedules = computed(() => list.data.value?.schedules ?? []);
const count = computed(() => list.data.value?.count);
const selected = ref<Schedule[]>([]);

watch(schedules, () => {
  const ids = new Set(schedules.value.map(s => s.id));
  selected.value = selected.value.filter(s => ids.has(s.id));
});

const hasFilters = computed(() => {
  const f = effectiveFilters.value;
  return (
    f.scheduleIds.length + f.jobTypeIds.length + f.statuses.length + f.labels.length > 0 ||
    !!f.createdAt
  );
});

/** Pending stop request, shown in a dialog. */
const stopRequest = ref<StopSchedulesRequest>();

function requestStop(stopFilters: ScheduleFilters, message: string) {
  stopRequest.value = { filters: stopFilters, message };
}

function stopSelected() {
  requestStop(
    create(ScheduleFiltersSchema, { scheduleIds: selected.value.map(s => s.id) }),
    `Stop ${selected.value.length} selected schedule(s)?`,
  );
}

function stopMatching() {
  requestStop(
    effectiveFilters.value,
    hasFilters.value
      ? `Stop all active schedules matching the current filters (up to ${count.value ?? "?"} schedules)?`
      : "No filters are set, this will stop ALL active schedules. Are you sure?",
  );
}

function stopOne(schedule: Schedule) {
  requestStop(
    create(ScheduleFiltersSchema, { scheduleIds: [schedule.id] }),
    `Stop schedule ${schedule.id}?`,
  );
}

defineExpose({ reload: list.reload });
</script>

<template>
  <DataTable
    v-model:selection="selected"
    :value="schedules"
    data-key="id"
    lazy
    :paginator="schedules.length > 0 || pagination.first.value > 0"
    :rows="pagination.rows.value"
    :first="pagination.first.value"
    :total-records="pagination.totalRecords(count, schedules.length)"
    :rows-per-page-options="compact ? undefined : [10, 20, 50, 100]"
    :paginator-template="pagination.paginatorTemplate"
    current-page-report-template="{first} – {last}"
    :loading="list.loading.value"
    :size="compact ? 'small' : undefined"
    scrollable
    @page="pagination.onPage"
  >
    <template #header>
      <div class="flex flex-col gap-3">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <span class="text-sm text-muted-color">
              {{ count ?? "…" }} schedule{{ count === 1 ? "" : "s" }}
            </span>
            <template v-if="!compact">
              <Button
                v-if="selected.length > 0"
                :label="`Stop selected (${selected.length})`"
                icon="pi pi-stop-circle"
                severity="danger"
                outlined
                size="small"
                @click="stopSelected"
              />
              <Button
                label="Stop matching"
                icon="pi pi-stop-circle"
                severity="danger"
                text
                size="small"
                :disabled="count === 0"
                @click="stopMatching"
              />
            </template>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <Select
              v-if="!compact"
              v-model="orderBy"
              :options="orderOptions"
              option-label="label"
              option-value="value"
              size="small"
            />
            <RefreshControl
              v-if="!compact"
              id="schedules"
              :loading="list.loading.value"
              @refresh="list.reload"
            />
          </div>
        </div>
        <ScheduleFilters v-if="!compact" v-model="filters" :hidden="baseKeys" />
      </div>
    </template>

    <template #empty>
      <div class="py-6 text-center text-muted-color">No schedules found.</div>
    </template>

    <Column v-if="!compact" selection-mode="multiple" header-style="width: 3rem" />
    <Column header="ID">
      <template #body="{ data }">
        <CopyableId :id="data.id" short :to="`/schedules/${data.id}`" />
      </template>
    </Column>
    <Column v-if="!baseKeys.includes('jobTypeIds')" header="Job type">
      <template #body="{ data }">
        <RouterLink
          :to="`/job-types/${data.schedule?.jobTemplate?.jobTypeId}`"
          class="hover:underline"
        >
          {{ data.schedule?.jobTemplate?.jobTypeId }}
        </RouterLink>
      </template>
    </Column>
    <Column header="Scheduling">
      <template #body="{ data }">
        <span class="font-mono text-sm">{{ schedulingSummary(data.schedule?.scheduling) }}</span>
      </template>
    </Column>
    <Column header="Status">
      <template #body="{ data }">
        <Tag
          :value="scheduleStatusInfo[data.status as ScheduleStatus].label"
          :severity="scheduleStatusInfo[data.status as ScheduleStatus].severity"
          :icon="scheduleStatusInfo[data.status as ScheduleStatus].icon"
          class="whitespace-nowrap"
        />
      </template>
    </Column>
    <Column header="Created">
      <template #body="{ data }">
        <span v-tooltip.top="formatTimestamp(data.createdAt)" class="whitespace-nowrap">
          {{ formatRelative(data.createdAt) }}
        </span>
      </template>
    </Column>
    <Column v-if="!compact" header="Stopped">
      <template #body="{ data }">
        <span v-tooltip.top="formatTimestamp(data.stoppedAt)" class="whitespace-nowrap">
          {{ formatRelative(data.stoppedAt) }}
        </span>
      </template>
    </Column>
    <Column v-if="!compact" header="Labels">
      <template #body="{ data }">
        <div class="flex flex-wrap gap-1">
          <Tag
            v-for="label in data.schedule?.labels"
            :key="label.key"
            :value="`${label.key}=${label.value}`"
            severity="secondary"
            class="font-normal! whitespace-nowrap"
          />
        </div>
      </template>
    </Column>
    <Column v-if="!compact" header-style="width: 3rem">
      <template #body="{ data }">
        <Button
          v-if="data.status === ScheduleStatus.ACTIVE"
          v-tooltip.left="'Stop schedule'"
          icon="pi pi-stop-circle"
          severity="danger"
          text
          rounded
          size="small"
          aria-label="Stop schedule"
          @click="stopOne(data)"
        />
      </template>
    </Column>
  </DataTable>

  <StopSchedulesDialog v-model="stopRequest" @stopped="list.reload" />
</template>
