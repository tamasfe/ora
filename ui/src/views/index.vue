<script setup lang="ts">
import { computed, ref } from "vue";
import { ExecutionStatus } from "../api/ora/admin/v1/executions_pb";
import { ScheduleStatus } from "../api/ora/admin/v1/schedules_pb";
import { useOraAdminClient } from "../grpc";
import { useLoader } from "../util";
import { useExecutors, useJobTypes } from "../util/data";
import { jobsLink, schedulesLink } from "../util/route";
import { executionStatusOptions } from "../util/status";

const client = useOraAdminClient();
const { jobTypes } = useJobTypes();
const executors = useExecutors();

const counts = useLoader(async signal => {
  const [total, byStatus, activeSchedules] = await Promise.all([
    client.countJobs({}, { signal }),
    Promise.all(
      executionStatusOptions.map(option =>
        client.countJobs({ filters: { executionStatuses: [option.value] } }, { signal }),
      ),
    ),
    client.countSchedules({ filters: { statuses: [ScheduleStatus.ACTIVE] } }, { signal }),
  ]);

  return {
    total: Number(total.count),
    byStatus: byStatus.map(res => Number(res.count)),
    activeSchedules: Number(activeSchedules.count),
  };
});

const failedJobsTable = ref<{ reload(): void }>();
const activeSchedulesTable = ref<{ reload(): void }>();

function refresh() {
  counts.reload();
  executors.reload();
  failedJobsTable.value?.reload();
  activeSchedulesTable.value?.reload();
}

const statusCards = computed(() =>
  executionStatusOptions.map((option, index) => ({
    ...option,
    count: counts.data.value?.byStatus[index],
    to: jobsLink({ executionStatuses: [option.value] }),
  })),
);

const failedFilters = { executionStatuses: [ExecutionStatus.FAILED] };
const activeScheduleFilters = { statuses: [ScheduleStatus.ACTIVE] };
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between gap-2">
      <h1 class="text-2xl font-semibold">Dashboard</h1>
      <RefreshControl :loading="counts.loading.value" @refresh="refresh" />
    </div>

    <div class="grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
      <RouterLink to="/jobs">
        <Card class="h-full transition-shadow hover:shadow-md">
          <template #content>
            <div class="flex flex-col gap-1">
              <span class="text-sm text-muted-color"
                ><i class="pi pi-briefcase mr-1" />All jobs</span
              >
              <span class="text-3xl font-semibold">{{ counts.data.value?.total ?? "…" }}</span>
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
              <span class="text-3xl font-semibold">{{ card.count ?? "…" }}</span>
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
              <span class="text-3xl font-semibold">{{
                counts.data.value?.activeSchedules ?? "…"
              }}</span>
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
              <span class="text-3xl font-semibold">{{ executors.executors.value.length }}</span>
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
              <span class="text-3xl font-semibold">{{ jobTypes.length }}</span>
            </div>
          </template>
        </Card>
      </RouterLink>
    </div>

    <div class="grid grid-cols-1 gap-4 xl:grid-cols-2">
      <div class="flex flex-col gap-2">
        <div class="flex items-center justify-between">
          <h2 class="text-xl font-semibold">Recently failed jobs</h2>
          <RouterLink :to="jobsLink(failedFilters)" class="text-sm text-primary hover:underline"
            >View all</RouterLink
          >
        </div>
        <JobsTable ref="failedJobsTable" :base-filters="failedFilters" compact :rows="5" />
      </div>
      <div class="flex flex-col gap-2">
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
        />
      </div>
    </div>
  </div>
</template>
