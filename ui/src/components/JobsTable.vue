<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { create, toJsonString, type MessageInitShape } from "@bufbuild/protobuf";
import { useConfirm } from "primevue/useconfirm";
import { useToast } from "primevue/usetoast";
import {
  JobFiltersSchema,
  JobOrderBy,
  type Job,
  type JobFilters,
} from "../api/ora/admin/v1/jobs_pb";
import { LabelFilterSchema, type Label } from "../api/ora/common/v1/label_pb";
import { useOraAdminClient } from "../grpc";
import { useLoader } from "../util";
import { useErrorToast } from "../util/errors";
import {
  formatClock,
  formatCount,
  formatLatency,
  formatRelative,
  formatSeconds,
  formatTimestamp,
} from "../util/format";
import { withLabelFilter } from "../util/labels";
import { pageSizeOptions, useTokenPagination } from "../util/pagination";
import { usePolling } from "../util/polling";
import { param, useRouteQuery, useRouteQueryFields } from "../util/query";
import { jobFilterFields, jobOrders, jobsLink, queryKey, useRefreshInterval } from "../util/route";
import {
  executionStatusInfo,
  isJobActive,
  jobEndedAt,
  jobStartedAt,
  jobStatus,
} from "../util/status";
import { useStopwatch } from "../util/time";

const props = withDefaults(
  defineProps<{
    /** Fixed filters that are always applied and cannot be changed. */
    baseFilters?: MessageInitShape<typeof JobFiltersSchema>;
    /** Hide the filters and bulk actions, and don't count the jobs. */
    compact?: boolean;
    /** The default page size. */
    rows?: number;
    /**
     * Keeps the filters and settings in the route query, with names prefixed by this value
     * (e.g. `jobs.` for tables embedded in other pages). The state is local if not set.
     */
    queryPrefix?: string;
    /** Auto refresh interval controlled by the page, hides the table's own refresh control. */
    refreshInterval?: number;
  }>(),
  { rows: 20 },
);

const route = useRoute();
const client = useOraAdminClient();
const confirm = useConfirm();
const toast = useToast();
const reportError = useErrorToast();

const prefix = props.queryPrefix;
const filters = useRouteQueryFields(prefix, jobFilterFields, () => create(JobFiltersSchema));
const orderBy = useRouteQuery(
  queryKey(prefix, "order"),
  param.oneOf(jobOrders),
  JobOrderBy.CREATED_AT_DESC,
);
const rows = useRouteQuery(queryKey(prefix, "rows"), param.int(pageSizeOptions), props.rows);
const ownRefresh = useRefreshInterval(
  props.refreshInterval === undefined ? queryKey(prefix, "refresh") : undefined,
);
const refresh = computed(() => props.refreshInterval ?? ownRefresh.value);

const orderOptions = [
  { label: "Newest first", value: JobOrderBy.CREATED_AT_DESC },
  { label: "Oldest first", value: JobOrderBy.CREATED_AT_ASC },
  { label: "Target time (latest first)", value: JobOrderBy.TARGET_EXECUTION_TIME_DESC },
  { label: "Target time (earliest first)", value: JobOrderBy.TARGET_EXECUTION_TIME_ASC },
  { label: "Priority (highest first)", value: JobOrderBy.PRIORITY_DESC },
  { label: "Priority (lowest first)", value: JobOrderBy.PRIORITY_ASC },
];

// Parents often pass inline objects, compare by value to avoid needless reloads.
const baseFiltersJson = computed(() => JSON.stringify(props.baseFilters ?? {}));
const baseFilters = computed(() => create(JobFiltersSchema, JSON.parse(baseFiltersJson.value)));
const baseKeys = computed(
  () => Object.keys(JSON.parse(baseFiltersJson.value)) as (keyof JobFilters)[],
);

/** The filters sent to the server. */
const effectiveFilters = computed<JobFilters>(() => {
  const result = { ...filters.value };
  for (const key of baseKeys.value) {
    (result as any)[key] = baseFilters.value[key];
  }
  return result;
});
const filtersKey = computed(() => toJsonString(JobFiltersSchema, effectiveFilters.value));

// Counting can be slow for many jobs, so it is loaded separately from the list and only
// when the filters change, the list doesn't wait for it.
const count = useLoader(
  () => (props.compact ? undefined : effectiveFilters.value),
  async (requestFilters, signal) =>
    Number((await client.countJobs({ filters: requestFilters }, { signal })).count),
  { key: requestFilters => toJsonString(JobFiltersSchema, requestFilters) },
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
  async (request, signal): Promise<Job[]> => {
    const res = await client.listJobs(
      {
        filters: request.filters,
        orderBy: request.orderBy,
        pagination: { pageSize: request.pageSize, nextPageToken: request.pageToken },
      },
      { signal },
    );

    if (signal.aborted) {
      return res.jobs;
    }

    pagination.update(request.key, request.page, res.jobs.length, res.nextPageToken);

    if (res.jobs.length === 0 && request.page > 0) {
      // The previous page was the last one, it is shown again.
      return list.data.value ?? [];
    }

    return res.jobs;
  },
  { key: request => `${request.key}|${request.page}|${request.pageToken ?? ""}` },
);

usePolling([list, count], refresh);

function reload(force = false) {
  list.reload(force);
  count.reload(force);
}

const jobs = computed(() => list.data.value ?? []);
const selected = ref<Job[]>([]);

watch(jobs, () => {
  const ids = new Set(jobs.value.map(job => job.id));
  selected.value = selected.value.filter(job => ids.has(job.id));
});

const busy = computed(() => list.busy.value || count.busy.value);
/** The rows belong to a previous page or filters, and the new ones are taking a while. */
const fading = computed(() => list.stale.value && list.busy.value);
const firstLoad = computed(() => list.data.value === undefined && !list.error.value);

/** Whether filters are set by the user, not only the fixed base filters. */
const hasUserFilters = computed(() => toJsonString(JobFiltersSchema, filters.value) !== "{}");

const hasFilters = computed(() => {
  const f = effectiveFilters.value;
  return (
    f.jobIds.length +
      f.jobTypeIds.length +
      f.scheduleIds.length +
      f.executorIds.length +
      f.executionIds.length +
      f.executionStatuses.length +
      f.labels.length >
      0 ||
    !!f.targetExecutionTime ||
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
  return jobsLink({ labels: [{ key: label.key, value: label.value }] });
}

const cancelling = useStopwatch();

/** Bulk actions need the current count to be confirmed. */
const cancelMatchingDisabled = computed(
  () =>
    knownCount.value === undefined ||
    knownCount.value === 0 ||
    count.busy.value ||
    list.stale.value ||
    cancelling.running.value,
);

function cancelJobs(cancelFilters: JobFilters, message: string) {
  confirm.require({
    header: "Cancel jobs",
    message,
    icon: "pi pi-exclamation-triangle",
    rejectProps: { label: "Keep", severity: "secondary", outlined: true },
    acceptProps: { label: "Cancel jobs", severity: "danger" },
    accept: async () => {
      try {
        const { result, ms } = await cancelling.time(() =>
          client.cancelJobs({ filters: cancelFilters }),
        );
        toast.add({
          severity: "success",
          summary: "Jobs cancelled",
          detail: `${formatCount(result.cancelledJobIds.length)} job(s) cancelled in ${formatLatency(ms)}.`,
          life: 5000,
        });
        reload(true);
      } catch (error) {
        reportError(error, "Failed to cancel jobs");
      }
    },
  });
}

function cancelSelected() {
  cancelJobs(
    create(JobFiltersSchema, { jobIds: selected.value.map(job => job.id) }),
    `Cancel ${selected.value.length} selected job(s)? Only active jobs are affected.`,
  );
}

function cancelMatching() {
  const message = hasFilters.value
    ? `Cancel all active jobs matching the current filters (up to ${formatCount(knownCount.value ?? 0)} jobs)?`
    : "No filters are set, this will cancel ALL active jobs. Are you sure?";
  cancelJobs(effectiveFilters.value, message);
}

function cancelOne(job: Job) {
  cancelJobs(create(JobFiltersSchema, { jobIds: [job.id] }), `Cancel job ${job.id}?`);
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
      :value="jobs"
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
      :table-style="{ tableLayout: 'fixed', minWidth: compact ? '40rem' : '78rem' }"
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
                  job{{ knownCount === 1 ? "" : "s" }}
                </template>
              </div>
              <Button
                :label="`Cancel selected (${selected.length})`"
                icon="pi pi-ban"
                severity="danger"
                outlined
                size="small"
                :disabled="selected.length === 0 || cancelling.running.value"
                @click="cancelSelected"
              />
              <Button
                :label="
                  cancelling.running.value && (cancelling.elapsed.value ?? 0) >= 1000
                    ? `Cancelling… ${formatSeconds(cancelling.elapsed.value ?? 0)}`
                    : 'Cancel matching'
                "
                icon="pi pi-ban"
                severity="danger"
                text
                size="small"
                :loading="cancelling.running.value"
                :disabled="cancelMatchingDisabled"
                @click="cancelMatching"
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
          <JobFilters v-model="filters" :hidden="baseKeys" />
        </div>
      </template>

      <template #empty>
        <div v-if="firstLoad" class="flex flex-col gap-4 py-2" aria-busy="true">
          <Skeleton v-for="i in Math.min(rows, 20)" :key="i" height="2rem" />
        </div>
        <div v-else-if="list.data.value === undefined" class="py-6 text-center text-red-500">
          The jobs could not be loaded.
        </div>
        <div v-else class="py-6 text-center text-muted-color">
          {{ hasUserFilters ? "No jobs match the filters." : "No jobs found." }}
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
          <CopyableId :id="data.id" short :to="`/jobs/${data.id}`" />
        </template>
      </Column>
      <Column :header="showJobType ? 'Job' : 'Labels'">
        <template #body="{ data }">
          <div class="flex min-w-0 flex-col gap-1">
            <RouterLink
              v-if="showJobType"
              v-tooltip.top="{ value: data.job?.jobTypeId, showDelay: 400 }"
              :to="`/job-types/${data.job?.jobTypeId}`"
              class="truncate font-mono text-sm hover:underline"
            >
              {{ data.job?.jobTypeId }}
            </RouterLink>
            <LabelList
              v-if="data.job?.labels.length"
              :labels="data.job.labels"
              :max="compact ? 2 : 4"
              :selectable="labelsFilterable"
              :link="labelsFilterable ? undefined : labelLink"
              :hint="
                labelsFilterable ? 'Click to filter by this label' : 'Show jobs with this label'
              "
              @select="filterByLabel"
            />
            <span v-else-if="!showJobType" class="text-muted-color">-</span>
          </div>
        </template>
      </Column>
      <Column header="Status" header-style="width: 9rem">
        <template #body="{ data }">
          <Tag
            :value="executionStatusInfo[jobStatus(data)].label"
            :severity="executionStatusInfo[jobStatus(data)].severity"
            :icon="executionStatusInfo[jobStatus(data)].icon"
            class="whitespace-nowrap"
          />
        </template>
      </Column>
      <Column header="Duration" header-style="width: 7rem">
        <template #body="{ data }">
          <ElapsedTime
            :start="jobStartedAt(data)"
            :end="jobEndedAt(data)"
            :live="isJobActive(data)"
            class="text-sm"
          />
        </template>
      </Column>
      <Column v-if="!compact" header="Priority" header-style="width: 6rem">
        <template #body="{ data }">
          <span class="tabular-nums" :class="{ 'text-muted-color': !data.job?.priority }">
            {{ data.job?.priority ?? 0 }}
          </span>
        </template>
      </Column>
      <Column v-if="!compact" header="Attempts" header-style="width: 6rem">
        <template #body="{ data }">
          <span class="tabular-nums">{{ data.executions.length }}</span>
        </template>
      </Column>
      <Column v-if="!compact" header="Target time" header-style="width: 13rem">
        <template #body="{ data }">
          <span
            v-tooltip.top="
              `${formatTimestamp(data.job?.targetExecutionTime)} (${formatRelative(data.job?.targetExecutionTime)})`
            "
            class="block truncate text-sm tabular-nums"
          >
            {{ formatTimestamp(data.job?.targetExecutionTime) }}
          </span>
        </template>
      </Column>
      <Column header="Created" header-style="width: 9rem">
        <template #body="{ data }">
          <span v-tooltip.top="formatTimestamp(data.createdAt)" class="block truncate text-sm">
            {{ formatRelative(data.createdAt) }}
          </span>
        </template>
      </Column>
      <Column
        v-if="!compact && !baseKeys.includes('scheduleIds')"
        header="Schedule"
        header-style="width: 8rem"
      >
        <template #body="{ data }">
          <CopyableId
            v-if="data.scheduleId"
            :id="data.scheduleId"
            short
            :to="`/schedules/${data.scheduleId}`"
          />
          <span v-else class="text-muted-color">-</span>
        </template>
      </Column>
      <Column v-if="!compact" frozen align-frozen="right" header-style="width: 3.5rem">
        <template #body="{ data }">
          <Button
            v-if="isJobActive(data)"
            v-tooltip.left="'Cancel job'"
            icon="pi pi-ban"
            severity="danger"
            text
            rounded
            size="small"
            aria-label="Cancel job"
            :disabled="cancelling.running.value"
            @click="cancelOne(data)"
          />
        </template>
      </Column>
    </DataTable>
  </div>
</template>
