<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { create, type MessageInitShape } from "@bufbuild/protobuf";
import { useConfirm } from "primevue/useconfirm";
import { useToast } from "primevue/usetoast";
import {
  JobFiltersSchema,
  JobOrderBy,
  type Job,
  type JobFilters,
} from "../api/ora/admin/v1/jobs_pb";
import { useOraAdminClient } from "../grpc";
import { useLoader } from "../util";
import { useErrorToast } from "../util/errors";
import { formatRelative, formatTimestamp } from "../util/format";
import { useTokenPagination } from "../util/pagination";
import { executionStatusInfo, isJobActive, jobStatus } from "../util/status";

const props = withDefaults(
  defineProps<{
    /** Fixed filters that are always applied and cannot be changed. */
    baseFilters?: MessageInitShape<typeof JobFiltersSchema>;
    /** Hide the filters and bulk actions. */
    compact?: boolean;
    rows?: number;
  }>(),
  { rows: 20 },
);

const filters = defineModel<JobFilters>("filters", {
  default: () => create(JobFiltersSchema),
});

const client = useOraAdminClient();
const confirm = useConfirm();
const toast = useToast();
const reportError = useErrorToast();

const orderBy = ref(JobOrderBy.CREATED_AT_DESC);
const orderOptions = [
  { label: "Newest first", value: JobOrderBy.CREATED_AT_DESC },
  { label: "Oldest first", value: JobOrderBy.CREATED_AT_ASC },
  { label: "Target time (latest first)", value: JobOrderBy.TARGET_EXECUTION_TIME_DESC },
  { label: "Target time (earliest first)", value: JobOrderBy.TARGET_EXECUTION_TIME_ASC },
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

const pagination = useTokenPagination(props.rows);

watch([effectiveFilters, orderBy], () => pagination.reset());

const list = useLoader(async signal => {
  const requestFilters = effectiveFilters.value;
  const listRequest = client.listJobs(
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
  const countRequest = client.countJobs({ filters: requestFilters }, { signal });

  const [res, count] = await Promise.all([listRequest, countRequest]);
  pagination.update(res.jobs.length, res.nextPageToken, Number(count.count));

  return { jobs: res.jobs, count: Number(count.count) };
});

const jobs = computed(() => list.data.value?.jobs ?? []);
const count = computed(() => list.data.value?.count);
const selected = ref<Job[]>([]);

watch(jobs, () => {
  const ids = new Set(jobs.value.map(job => job.id));
  selected.value = selected.value.filter(job => ids.has(job.id));
});

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

function cancelJobs(cancelFilters: JobFilters, message: string) {
  confirm.require({
    header: "Cancel jobs",
    message,
    icon: "pi pi-exclamation-triangle",
    rejectProps: { label: "Keep", severity: "secondary", outlined: true },
    acceptProps: { label: "Cancel jobs", severity: "danger" },
    accept: async () => {
      try {
        const res = await client.cancelJobs({ filters: cancelFilters });
        toast.add({
          severity: "success",
          summary: "Jobs cancelled",
          detail: `${res.cancelledJobIds.length} job(s) cancelled.`,
          life: 5000,
        });
        list.reload();
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
    ? `Cancel all active jobs matching the current filters (up to ${count.value ?? "?"} jobs)?`
    : "No filters are set, this will cancel ALL active jobs. Are you sure?";
  cancelJobs(effectiveFilters.value, message);
}

function cancelOne(job: Job) {
  cancelJobs(create(JobFiltersSchema, { jobIds: [job.id] }), `Cancel job ${job.id}?`);
}

defineExpose({ reload: list.reload });
</script>

<template>
  <DataTable
    v-model:selection="selected"
    :value="jobs"
    data-key="id"
    lazy
    :paginator="jobs.length > 0 || pagination.first.value > 0"
    :rows="pagination.rows.value"
    :first="pagination.first.value"
    :total-records="pagination.totalRecords(count, jobs.length)"
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
              {{ count ?? "…" }} job{{ count === 1 ? "" : "s" }}
            </span>
            <template v-if="!compact">
              <Button
                v-if="selected.length > 0"
                :label="`Cancel selected (${selected.length})`"
                icon="pi pi-ban"
                severity="danger"
                outlined
                size="small"
                @click="cancelSelected"
              />
              <Button
                label="Cancel matching"
                icon="pi pi-ban"
                severity="danger"
                text
                size="small"
                :disabled="count === 0"
                @click="cancelMatching"
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
              id="jobs"
              :loading="list.loading.value"
              @refresh="list.reload"
            />
          </div>
        </div>
        <JobFilters v-if="!compact" v-model="filters" :hidden="baseKeys" />
      </div>
    </template>

    <template #empty>
      <div class="py-6 text-center text-muted-color">No jobs found.</div>
    </template>

    <Column v-if="!compact" selection-mode="multiple" header-style="width: 3rem" />
    <Column header="ID">
      <template #body="{ data }">
        <CopyableId :id="data.id" short :to="`/jobs/${data.id}`" />
      </template>
    </Column>
    <Column v-if="!baseKeys.includes('jobTypeIds')" header="Job type">
      <template #body="{ data }">
        <RouterLink :to="`/job-types/${data.job?.jobTypeId}`" class="hover:underline">
          {{ data.job?.jobTypeId }}
        </RouterLink>
      </template>
    </Column>
    <Column header="Status">
      <template #body="{ data }">
        <Tag
          :value="executionStatusInfo[jobStatus(data)].label"
          :severity="executionStatusInfo[jobStatus(data)].severity"
          :icon="executionStatusInfo[jobStatus(data)].icon"
          class="whitespace-nowrap"
        />
      </template>
    </Column>
    <Column header="Attempts">
      <template #body="{ data }">{{ data.executions.length }}</template>
    </Column>
    <Column header="Target time">
      <template #body="{ data }">
        <span
          v-tooltip.top="formatRelative(data.job?.targetExecutionTime)"
          class="whitespace-nowrap"
        >
          {{ formatTimestamp(data.job?.targetExecutionTime) }}
        </span>
      </template>
    </Column>
    <Column header="Created">
      <template #body="{ data }">
        <span v-tooltip.top="formatTimestamp(data.createdAt)" class="whitespace-nowrap">
          {{ formatRelative(data.createdAt) }}
        </span>
      </template>
    </Column>
    <Column v-if="!compact && !baseKeys.includes('scheduleIds')" header="Schedule">
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
    <Column v-if="!compact" header="Labels">
      <template #body="{ data }">
        <div class="flex flex-wrap gap-1">
          <Tag
            v-for="label in data.job?.labels"
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
          v-if="isJobActive(data)"
          v-tooltip.left="'Cancel job'"
          icon="pi pi-ban"
          severity="danger"
          text
          rounded
          size="small"
          aria-label="Cancel job"
          @click="cancelOne(data)"
        />
      </template>
    </Column>
  </DataTable>
</template>
