<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { create, toJsonString, type MessageInitShape } from "@bufbuild/protobuf";
import {
  ScheduleFiltersSchema,
  ScheduleOrderBy,
  ScheduleStatus,
  type Schedule,
  type ScheduleFilters,
} from "../api/ora/admin/v1/schedules_pb";
import { LabelFilterSchema, type Label } from "../api/ora/common/v1/label_pb";
import { useOraAdminClient } from "../grpc";
import { useLoader } from "../util";
import { formatClock, formatCount, formatRelative, formatTimestamp } from "../util/format";
import { withLabelFilter } from "../util/labels";
import { pageSizeOptions, useTokenPagination } from "../util/pagination";
import { usePolling } from "../util/polling";
import { param, useRouteQuery, useRouteQueryFields } from "../util/query";
import {
  queryKey,
  scheduleFilterFields,
  scheduleOrders,
  schedulesLink,
  useRefreshInterval,
} from "../util/route";
import { schedulingSummary } from "../util/schedule";
import type { StopSchedulesRequest } from "./StopSchedulesDialog.vue";
import { scheduleStatusInfo } from "../util/status";

const props = withDefaults(
  defineProps<{
    /** Fixed filters that are always applied and cannot be changed. */
    baseFilters?: MessageInitShape<typeof ScheduleFiltersSchema>;
    /** Hide the filters and bulk actions, and don't count the schedules. */
    compact?: boolean;
    /** The default page size. */
    rows?: number;
    /**
     * Keeps the filters and settings in the route query, with names prefixed by this value
     * (e.g. `schedules.` for tables embedded in other pages). The state is local if not set.
     */
    queryPrefix?: string;
    /** Auto refresh interval controlled by the page, hides the table's own refresh control. */
    refreshInterval?: number;
  }>(),
  { rows: 20 },
);

const route = useRoute();
const client = useOraAdminClient();

const prefix = props.queryPrefix;
const filters = useRouteQueryFields(prefix, scheduleFilterFields, () =>
  create(ScheduleFiltersSchema),
);
const orderBy = useRouteQuery(
  queryKey(prefix, "order"),
  param.oneOf(scheduleOrders),
  ScheduleOrderBy.CREATED_AT_DESC,
);
const rows = useRouteQuery(queryKey(prefix, "rows"), param.int(pageSizeOptions), props.rows);
const ownRefresh = useRefreshInterval(
  props.refreshInterval === undefined ? queryKey(prefix, "refresh") : undefined,
);
const refresh = computed(() => props.refreshInterval ?? ownRefresh.value);

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
const filtersKey = computed(() => toJsonString(ScheduleFiltersSchema, effectiveFilters.value));

// Counting is loaded separately, so that the list doesn't wait for it.
const count = useLoader(
  () => (props.compact ? undefined : effectiveFilters.value),
  async (requestFilters, signal) =>
    Number((await client.countSchedules({ filters: requestFilters }, { signal })).count),
  { key: requestFilters => toJsonString(ScheduleFiltersSchema, requestFilters) },
);

/** The count for the current filters, unknown while it is loaded for new filters. */
const knownCount = computed(() => (count.stale.value ? undefined : count.data.value));

const pagination = useTokenPagination({
  key: computed(() => `${filtersKey.value}|${orderBy.value}|${rows.value}`),
  rows,
  count: knownCount,
  cacheId: prefix === undefined ? undefined : `${route.path}|${prefix}`,
});

const list = useLoader(
  () => ({
    filters: effectiveFilters.value,
    orderBy: orderBy.value,
    pageSize: rows.value,
    page: pagination.page.value,
    pageToken: pagination.pageToken.value,
    key: pagination.key.value,
  }),
  async (request, signal): Promise<Schedule[]> => {
    const res = await client.listSchedules(
      {
        filters: request.filters,
        orderBy: request.orderBy,
        pagination: { pageSize: request.pageSize, nextPageToken: request.pageToken },
      },
      { signal },
    );

    if (signal.aborted) {
      return res.schedules;
    }

    pagination.update(request.key, request.page, res.schedules.length, res.nextPageToken);

    if (res.schedules.length === 0 && request.page > 0) {
      // The previous page was the last one, it is shown again.
      return list.data.value ?? [];
    }

    return res.schedules;
  },
  { key: request => `${request.key}|${request.page}|${request.pageToken ?? ""}` },
);

usePolling([list, count], refresh);

function reload(force = false) {
  list.reload(force);
  count.reload(force);
}

const schedules = computed(() => list.data.value ?? []);
const selected = ref<Schedule[]>([]);

watch(schedules, () => {
  const ids = new Set(schedules.value.map(s => s.id));
  selected.value = selected.value.filter(s => ids.has(s.id));
});

const busy = computed(() => list.busy.value || count.busy.value);
/** The rows belong to a previous page or filters, and the new ones are taking a while. */
const fading = computed(() => list.stale.value && list.busy.value);
const firstLoad = computed(() => list.data.value === undefined && !list.error.value);

/** Whether filters are set by the user, not only the fixed base filters. */
const hasUserFilters = computed(() => toJsonString(ScheduleFiltersSchema, filters.value) !== "{}");

const hasFilters = computed(() => {
  const f = effectiveFilters.value;
  return (
    f.scheduleIds.length + f.jobTypeIds.length + f.statuses.length + f.labels.length > 0 ||
    !!f.createdAt
  );
});

const showJobType = computed(() => !baseKeys.value.includes("jobTypeIds"));
const labelsFilterable = computed(() => !props.compact && !baseKeys.value.includes("labels"));

function filterByLabel(label: Label) {
  filters.value = {
    ...filters.value,
    labels: withLabelFilter(
      filters.value.labels,
      create(LabelFilterSchema, { key: label.key, value: label.value }),
    ),
  };
}

function labelLink(label: Label) {
  return schedulesLink({ labels: [{ key: label.key, value: label.value }] });
}

/** Pending stop request, shown in a dialog. */
const stopRequest = ref<StopSchedulesRequest>();

/** Bulk actions need the current count to be confirmed. */
const stopMatchingDisabled = computed(
  () =>
    knownCount.value === undefined ||
    knownCount.value === 0 ||
    count.busy.value ||
    list.stale.value,
);

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
      ? `Stop all active schedules matching the current filters (up to ${formatCount(knownCount.value ?? 0)} schedules)?`
      : "No filters are set, this will stop ALL active schedules. Are you sure?",
  );
}

function stopOne(schedule: Schedule) {
  requestStop(
    create(ScheduleFiltersSchema, { scheduleIds: [schedule.id] }),
    `Stop schedule ${schedule.id}?`,
  );
}

function rowClass() {
  return fading.value ? "row-stale" : undefined;
}

defineExpose({ reload });
</script>

<template>
  <div class="relative">
    <LoadingBar :active="busy" class="absolute inset-x-0 top-0 z-10" />
    <DataTable
      v-model:selection="selected"
      :value="schedules"
      data-key="id"
      lazy
      paginator
      :rows="rows"
      :first="pagination.first.value"
      :total-records="pagination.totalRecords.value"
      :rows-per-page-options="compact ? undefined : pageSizeOptions"
      :paginator-template="pagination.paginatorTemplate"
      :current-page-report-template="pagination.report.value"
      :size="compact ? 'small' : undefined"
      :row-class="rowClass"
      :table-style="{ tableLayout: 'fixed', minWidth: compact ? '34rem' : '64rem' }"
      scrollable
      @page="pagination.onPage"
    >
      <template v-if="!compact" #header>
        <div class="flex flex-col gap-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex flex-wrap items-center gap-2">
              <div class="min-w-24 text-sm text-muted-color tabular-nums">
                <Skeleton
                  v-if="knownCount === undefined && !count.error.value"
                  width="5rem"
                  height="1.25rem"
                />
                <template v-else>
                  {{ knownCount === undefined ? "?" : formatCount(knownCount) }}
                  schedule{{ knownCount === 1 ? "" : "s" }}
                </template>
              </div>
              <Button
                :label="`Stop selected (${selected.length})`"
                icon="pi pi-stop-circle"
                severity="danger"
                outlined
                size="small"
                :disabled="selected.length === 0"
                @click="stopSelected"
              />
              <Button
                label="Stop matching"
                icon="pi pi-stop-circle"
                severity="danger"
                text
                size="small"
                :disabled="stopMatchingDisabled"
                @click="stopMatching"
              />
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <Select
                v-model="orderBy"
                :options="orderOptions"
                option-label="label"
                option-value="value"
                size="small"
                aria-label="Order"
              />
              <RefreshControl
                v-if="refreshInterval === undefined"
                v-model="ownRefresh"
                :loading="busy"
                @refresh="reload()"
              />
            </div>
          </div>
          <ScheduleFilters v-model="filters" :hidden="baseKeys" />
        </div>
      </template>

      <template #empty>
        <div v-if="firstLoad" class="flex flex-col gap-4 py-2" aria-busy="true">
          <Skeleton v-for="i in Math.min(rows, 20)" :key="i" height="2rem" />
        </div>
        <div v-else-if="list.data.value === undefined" class="py-6 text-center text-red-500">
          The schedules could not be loaded.
        </div>
        <div v-else class="py-6 text-center text-muted-color">
          {{ hasUserFilters ? "No schedules match the filters." : "No schedules found." }}
        </div>
      </template>

      <!-- Both sides have the same width, so that the page links don't move. -->
      <template #paginatorstart>
        <div class="flex items-center gap-3" :class="compact ? 'w-32' : 'w-40 sm:w-72'">
          <LoadStatus :state="list" verb="list" icon />
          <LoadStatus :state="count" verb="count" icon />
        </div>
      </template>
      <template #paginatorend>
        <div
          class="text-right text-xs text-muted-color tabular-nums"
          :class="compact ? 'w-32' : 'w-40 sm:w-72'"
        >
          <template v-if="!compact && list.loadedAt.value !== undefined">
            Updated {{ formatClock(list.loadedAt.value) }}
          </template>
        </div>
      </template>

      <Column v-if="!compact" selection-mode="multiple" frozen header-style="width: 3rem" />
      <Column header="ID" header-style="width: 8rem">
        <template #body="{ data }">
          <CopyableId :id="data.id" short :to="`/schedules/${data.id}`" />
        </template>
      </Column>
      <Column :header="showJobType ? 'Schedule' : 'Labels'">
        <template #body="{ data }">
          <div class="flex min-w-0 flex-col gap-1">
            <RouterLink
              v-if="showJobType"
              v-tooltip.top="{ value: data.schedule?.jobTemplate?.jobTypeId, showDelay: 400 }"
              :to="`/job-types/${data.schedule?.jobTemplate?.jobTypeId}`"
              class="truncate font-mono text-sm hover:underline"
            >
              {{ data.schedule?.jobTemplate?.jobTypeId }}
            </RouterLink>
            <LabelList
              v-if="data.schedule?.labels.length"
              :labels="data.schedule.labels"
              :max="compact ? 2 : 4"
              :selectable="labelsFilterable"
              :link="labelsFilterable ? undefined : labelLink"
              :hint="
                labelsFilterable
                  ? 'Click to filter by this label'
                  : 'Show schedules with this label'
              "
              @select="filterByLabel"
            />
            <span v-else-if="!showJobType" class="text-muted-color">-</span>
          </div>
        </template>
      </Column>
      <Column header="Scheduling" :header-style="compact ? 'width: 10rem' : 'width: 13rem'">
        <template #body="{ data }">
          <span
            v-tooltip.top="{ value: schedulingSummary(data.schedule?.scheduling), showDelay: 400 }"
            class="block truncate font-mono text-sm"
          >
            {{ schedulingSummary(data.schedule?.scheduling) }}
          </span>
        </template>
      </Column>
      <Column header="Status" header-style="width: 8rem">
        <template #body="{ data }">
          <Tag
            :value="scheduleStatusInfo[data.status as ScheduleStatus].label"
            :severity="scheduleStatusInfo[data.status as ScheduleStatus].severity"
            :icon="scheduleStatusInfo[data.status as ScheduleStatus].icon"
            class="whitespace-nowrap"
          />
        </template>
      </Column>
      <Column v-if="!compact" header="Created" header-style="width: 9rem">
        <template #body="{ data }">
          <span v-tooltip.top="formatTimestamp(data.createdAt)" class="block truncate text-sm">
            {{ formatRelative(data.createdAt) }}
          </span>
        </template>
      </Column>
      <Column v-if="!compact" header="Stopped" header-style="width: 9rem">
        <template #body="{ data }">
          <span v-tooltip.top="formatTimestamp(data.stoppedAt)" class="block truncate text-sm">
            {{ formatRelative(data.stoppedAt) }}
          </span>
        </template>
      </Column>
      <Column v-if="!compact" frozen align-frozen="right" header-style="width: 3.5rem">
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
  </div>

  <StopSchedulesDialog v-model="stopRequest" @stopped="reload(true)" />
</template>
