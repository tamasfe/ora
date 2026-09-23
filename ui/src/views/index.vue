<script setup lang="ts">
import { computed, ref } from "vue";
import type { MessageInitShape } from "@bufbuild/protobuf";
import { ExecutionStatus } from "../api/ora/admin/v1/executions_pb";
import type { JobFiltersSchema } from "../api/ora/admin/v1/jobs_pb";
import { ScheduleStatus } from "../api/ora/admin/v1/schedules_pb";
import { useOraAdminClient } from "../grpc";
import { useLoader, type LoadState } from "../util";
import { useExecutors, useJobTypes } from "../util/data";
import { usePolling } from "../util/polling";
import { jobsLink, schedulesLink, useRefreshInterval } from "../util/route";
import { executionStatusOptions } from "../util/status";

const client = useOraAdminClient();
const jobTypes = useJobTypes();
const executors = useExecutors();
const refresh = useRefreshInterval();

/** Counts are loaded separately, so that they are shown as soon as they are available. */
function useJobCount(filters: MessageInitShape<typeof JobFiltersSchema>) {
  return useLoader(
    () => ({}),
    async (_, signal) => Number((await client.countJobs({ filters }, { signal })).count),
  );
}

const totalJobs = useJobCount({});

const statusCards = executionStatusOptions.map(option => ({
  ...option,
  count: useJobCount({ executionStatuses: [option.value] }),
  to: jobsLink({ executionStatuses: [option.value] }),
}));

const activeScheduleFilters = { statuses: [ScheduleStatus.ACTIVE] };
const failedFilters = { executionStatuses: [ExecutionStatus.FAILED] };

const activeSchedules = useLoader(
  () => ({}),
  async (_, signal) =>
    Number((await client.countSchedules({ filters: activeScheduleFilters }, { signal })).count),
);

const counts: LoadState[] = [totalJobs, ...statusCards.map(card => card.count), activeSchedules];

usePolling([...counts, executors], refresh);

const busy = computed(() => [...counts, executors].some(state => state.busy.value));

const failedJobsTable = ref<{ reload(): void }>();
const activeSchedulesTable = ref<{ reload(): void }>();

function reload() {
  counts.forEach(state => state.reload());
  executors.reload();
  jobTypes.reload();
  failedJobsTable.value?.reload();
  activeSchedulesTable.value?.reload();
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h1 class="text-2xl font-semibold">Dashboard</h1>
      <RefreshControl v-model="refresh" :loading="busy" @refresh="reload" />
    </div>

    <div class="grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
      <RouterLink to="/jobs">
        <Card class="h-full transition-shadow hover:shadow-md">
          <template #content>
            <div class="flex flex-col gap-1">
              <span class="text-sm text-muted-color"
                ><i class="pi pi-briefcase mr-1" />All jobs</span
              >
              <StatNumber :value="totalJobs.data.value" :failed="!!totalJobs.error.value" />
              <LoadStatus :state="totalJobs" verb="count" />
            </div>
          </template>
        </Card>
      </RouterLink>
      <RouterLink v-for="card in statusCards" :key="card.value" :to="card.to">
        <Card class="h-full transition-shadow hover:shadow-md">
          <template #content>
            <div class="flex flex-col gap-1">
              <Tag
                :value="card.label"
                :severity="card.severity"
                :icon="card.icon"
                class="self-start"
              />
              <StatNumber :value="card.count.data.value" :failed="!!card.count.error.value" />
              <LoadStatus :state="card.count" verb="count" />
            </div>
          </template>
        </Card>
      </RouterLink>
    </div>

    <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
      <RouterLink :to="schedulesLink(activeScheduleFilters)">
        <Card class="h-full transition-shadow hover:shadow-md">
          <template #content>
            <div class="flex flex-col gap-1">
              <span class="text-sm text-muted-color"
                ><i class="pi pi-calendar-clock mr-1" />Active schedules</span
              >
              <StatNumber
                :value="activeSchedules.data.value"
                :failed="!!activeSchedules.error.value"
              />
              <LoadStatus :state="activeSchedules" verb="count" />
            </div>
          </template>
        </Card>
      </RouterLink>
      <RouterLink to="/executors">
        <Card class="h-full transition-shadow hover:shadow-md">
          <template #content>
            <div class="flex flex-col gap-1">
              <span class="text-sm text-muted-color"
                ><i class="pi pi-server mr-1" />Connected executors</span
              >
              <StatNumber
                :value="executors.loaded.value ? executors.executors.value.length : undefined"
                :failed="!!executors.error.value"
              />
              <LoadStatus :state="executors.state" verb="list" />
            </div>
          </template>
        </Card>
      </RouterLink>
      <RouterLink to="/job-types">
        <Card class="h-full transition-shadow hover:shadow-md">
          <template #content>
            <div class="flex flex-col gap-1">
              <span class="text-sm text-muted-color"
                ><i class="pi pi-sitemap mr-1" />Job types</span
              >
              <StatNumber
                :value="jobTypes.loaded.value ? jobTypes.jobTypes.value.length : undefined"
                :failed="!!jobTypes.error.value"
              />
              <LoadStatus :state="jobTypes.state" verb="list" />
            </div>
          </template>
        </Card>
      </RouterLink>
    </div>

    <div class="grid grid-cols-1 gap-4 xl:grid-cols-2">
      <div class="flex min-w-0 flex-col gap-2">
        <div class="flex items-center justify-between">
          <h2 class="text-xl font-semibold">Recently failed jobs</h2>
          <RouterLink :to="jobsLink(failedFilters)" class="text-sm text-primary hover:underline"
            >View all</RouterLink
          >
        </div>
        <JobsTable
          ref="failedJobsTable"
          :base-filters="failedFilters"
          compact
          :rows="5"
          :refresh-interval="refresh"
        />
      </div>
      <div class="flex min-w-0 flex-col gap-2">
        <div class="flex items-center justify-between">
          <h2 class="text-xl font-semibold">Active schedules</h2>
          <RouterLink
            :to="schedulesLink(activeScheduleFilters)"
            class="text-sm text-primary hover:underline"
          >
            View all
          </RouterLink>
        </div>
        <SchedulesTable
          ref="activeSchedulesTable"
          :base-filters="activeScheduleFilters"
          compact
          :rows="5"
          :refresh-interval="refresh"
        />
      </div>
    </div>
  </div>
</template>
