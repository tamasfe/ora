<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { ExecutionStatus } from "../../api/ora/admin/v1/executions_pb";
import { ScheduleStatus } from "../../api/ora/admin/v1/schedules_pb";
import { useCountsInterval, useJobTypeCounts } from "../../util/counts";
import { useExecutors, useJobTypes } from "../../util/data";
import { formatClock, formatCompactCount, formatCount, shortId } from "../../util/format";
import { pageSizeOptions } from "../../util/pagination";
import { usePolling } from "../../util/polling";
import { param, useRouteQuery } from "../../util/query";
import { jobsLink, schedulesLink, useRefreshInterval, useSearchQuery } from "../../util/route";

const jobTypes = useJobTypes();
const executors = useExecutors();
const refresh = useRefreshInterval();
const search = useSearchQuery();
const rows = useRouteQuery("rows", param.int(pageSizeOptions), 20);

// Job types appear as executors connect, always show the latest list here.
jobTypes.reload();

/** Executors shown for each job type, the rest can be found on the executors page. */
const maxExecutors = 3;

const filtered = computed(() => {
  const query = search.value.trim().toLowerCase();
  if (!query) {
    return jobTypes.jobTypes.value;
  }

  return jobTypes.jobTypes.value.filter(
    jobType =>
      jobType.id.toLowerCase().includes(query) ||
      jobType.description?.toLowerCase().includes(query),
  );
});

/** The index of the first row on the current page. */
const first = ref(0);
watch(search, () => (first.value = 0));

// Counting takes a request for each job type and status, only the visible job types are counted.
const counts = useJobTypeCounts(() =>
  filtered.value.slice(first.value, first.value + rows.value).map(jobType => jobType.id),
);

usePolling([jobTypes, executors], refresh);
usePolling(counts.state, useCountsInterval(refresh));

const busy = computed(() => jobTypes.busy.value || executors.busy.value || counts.state.busy.value);

function reload() {
  jobTypes.reload();
  executors.reload();
  counts.state.reload();
}

function activeSchedules(jobTypeId: string) {
  return counts.counts.value.get(jobTypeId)?.activeSchedules;
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <h1 class="text-2xl font-semibold">Job Types</h1>

    <div class="relative">
      <LoadingBar :active="busy" class="absolute inset-x-0 top-0 z-10" />
      <DataTable
        :value="filtered"
        data-key="id"
        paginator
        v-model:rows="rows"
        v-model:first="first"
        :rows-per-page-options="pageSizeOptions"
      >
        <template #header>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex flex-wrap items-center gap-3">
              <IconField>
                <InputIcon class="pi pi-search" />
                <InputText
                  v-model="search"
                  placeholder="Search by ID or description"
                  aria-label="Search job types"
                  class="w-80"
                />
              </IconField>
              <div class="min-w-32 text-sm text-muted-color tabular-nums">
                <Skeleton v-if="!jobTypes.loaded.value" width="6rem" height="1.25rem" />
                <template v-else-if="search.trim()">
                  {{ formatCount(filtered.length) }} of
                  {{ formatCount(jobTypes.jobTypes.value.length) }} job type(s)
                </template>
                <template v-else>{{ formatCount(filtered.length) }} job type(s)</template>
              </div>
            </div>
            <RefreshControl v-model="refresh" :loading="busy" @refresh="reload" />
          </div>
        </template>
        <template #empty>
          <div v-if="!jobTypes.loaded.value" class="flex flex-col gap-4 py-2" aria-busy="true">
            <Skeleton v-for="i in 5" :key="i" height="2rem" />
          </div>
          <div v-else class="py-6 text-center text-muted-color">
            {{
              search.trim()
                ? "No job types match the search."
                : "No job types are known to the server."
            }}
          </div>
        </template>

        <template #paginatorstart>
          <div class="flex w-40 items-center gap-3 sm:w-64">
            <LoadStatus :state="jobTypes.state" verb="list" icon />
            <LoadStatus :state="counts.state" verb="count" icon />
          </div>
        </template>
        <template #paginatorend>
          <div class="w-40 text-right text-xs text-muted-color tabular-nums sm:w-64">
            <template v-if="jobTypes.loadedAt.value !== undefined">
              Updated {{ formatClock(jobTypes.loadedAt.value) }}
            </template>
          </div>
        </template>

        <Column header="ID">
          <template #body="{ data }">
            <RouterLink
              :to="`/job-types/${data.id}`"
              class="font-mono text-primary hover:underline"
            >
              {{ data.id }}
            </RouterLink>
          </template>
        </Column>
        <Column header="Description" field="description" />
        <Column header="Executors">
          <template #body="{ data }">
            <div class="flex flex-wrap items-center gap-1">
              <RouterLink
                v-for="executor in (executors.byJobType.value.get(data.id) ?? []).slice(
                  0,
                  maxExecutors,
                )"
                :key="executor.id"
                :to="`/executors/${executor.id}`"
              >
                <Tag
                  :value="executor.name ?? shortId(executor.id)"
                  severity="secondary"
                  class="font-normal!"
                />
              </RouterLink>
              <RouterLink
                v-if="(executors.byJobType.value.get(data.id)?.length ?? 0) > maxExecutors"
                :to="{ path: '/executors', query: { q: data.id } }"
                class="text-xs text-primary hover:underline"
              >
                +{{ (executors.byJobType.value.get(data.id)?.length ?? 0) - maxExecutors }} more
              </RouterLink>
              <Skeleton v-if="!executors.loaded.value" width="5rem" height="1.5rem" />
              <Tag
                v-else-if="!executors.byJobType.value.get(data.id)"
                value="None connected"
                severity="warn"
              />
            </div>
          </template>
        </Column>
        <Column header="Jobs">
          <template #body="{ data }">
            <JobStatusCounts
              nowrap
              :counts="counts.counts.value.get(data.id)?.jobs"
              :failed="!!counts.state.error.value"
              :link="
                (status: ExecutionStatus) =>
                  jobsLink({ jobTypeIds: [data.id], executionStatuses: [status] })
              "
            />
          </template>
        </Column>
        <Column header="Active schedules" header-class="whitespace-nowrap">
          <template #body="{ data }">
            <RouterLink
              v-if="activeSchedules(data.id) !== undefined"
              v-tooltip.top="`${formatCount(activeSchedules(data.id) ?? 0)} active schedule(s)`"
              :to="schedulesLink({ jobTypeIds: [data.id], statuses: [ScheduleStatus.ACTIVE] })"
              class="text-primary tabular-nums hover:underline"
            >
              {{ formatCompactCount(activeSchedules(data.id) ?? 0) }}
            </RouterLink>
            <span v-else-if="counts.state.error.value" class="text-muted-color">–</span>
            <Skeleton v-else width="2rem" />
          </template>
        </Column>
        <Column header-class="w-0">
          <template #body="{ data }">
            <div class="flex justify-end gap-1 whitespace-nowrap">
              <RouterLink v-slot="{ navigate }" :to="jobsLink({ jobTypeIds: [data.id] })" custom>
                <Button label="Jobs" severity="secondary" text size="small" @click="navigate" />
              </RouterLink>
              <RouterLink
                v-slot="{ navigate }"
                :to="schedulesLink({ jobTypeIds: [data.id] })"
                custom
              >
                <Button
                  label="Schedules"
                  severity="secondary"
                  text
                  size="small"
                  @click="navigate"
                />
              </RouterLink>
              <RouterLink
                v-slot="{ navigate }"
                :to="{ path: '/jobs/new', query: { jobType: data.id } }"
                custom
              >
                <Button label="New job" icon="pi pi-plus" size="small" outlined @click="navigate" />
              </RouterLink>
              <RouterLink
                v-slot="{ navigate }"
                :to="{ path: '/schedules/new', query: { jobType: data.id } }"
                custom
              >
                <Button
                  label="New schedule"
                  icon="pi pi-calendar-plus"
                  size="small"
                  outlined
                  @click="navigate"
                />
              </RouterLink>
            </div>
          </template>
        </Column>
      </DataTable>
    </div>
  </div>
</template>
