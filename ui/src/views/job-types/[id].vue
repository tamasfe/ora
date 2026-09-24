<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import type { ExecutionStatus } from "../../api/ora/admin/v1/executions_pb";
import { ScheduleStatus } from "../../api/ora/admin/v1/schedules_pb";
import { useCountsInterval, useJobTypeCounts } from "../../util/counts";
import { useExecutors, useJobTypes } from "../../util/data";
import { formatCount, prettyJson, shortId } from "../../util/format";
import { usePolling } from "../../util/polling";
import { param, useRouteQuery } from "../../util/query";
import { jobsLink, schedulesLink, useRefreshInterval } from "../../util/route";

const route = useRoute("/job-types/[id]");
const router = useRouter();

const id = computed(() => route.params.id);
const refresh = useRefreshInterval();
const tab = useRouteQuery("tab", param.oneOf({ jobs: "jobs", schedules: "schedules" }), "jobs");

const jobTypes = useJobTypes();
const executors = useExecutors();
const counts = useJobTypeCounts(() => [id.value]);

usePolling(executors, refresh);
usePolling(counts.state, useCountsInterval(refresh));

const jobTypeCounts = computed(() => counts.counts.value.get(id.value));

const jobType = computed(() => jobTypes.byId.value.get(id.value));
const jobTypeExecutors = computed(() => executors.byJobType.value.get(id.value) ?? []);
const baseFilters = computed(() => ({ jobTypeIds: [id.value] }));

/** Executors shown, the rest can be found on the executors page. */
const maxExecutors = 5;

const jobsTable = ref<{ reload(): void }>();
const schedulesTable = ref<{ reload(): void }>();

function reload() {
  jobTypes.reload();
  executors.reload();
  counts.state.reload();
  jobsTable.value?.reload();
  schedulesTable.value?.reload();
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
        <h1 class="truncate font-mono text-2xl font-semibold">{{ id }}</h1>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <RefreshControl
          v-model="refresh"
          :loading="jobTypes.busy.value || executors.busy.value || counts.state.busy.value"
          @refresh="reload"
        />
        <RouterLink
          v-slot="{ navigate }"
          :to="{ path: '/schedules/new', query: { jobType: id } }"
          custom
        >
          <Button
            label="New schedule"
            icon="pi pi-calendar-plus"
            severity="secondary"
            outlined
            @click="navigate"
          />
        </RouterLink>
        <RouterLink
          v-slot="{ navigate }"
          :to="{ path: '/jobs/new', query: { jobType: id } }"
          custom
        >
          <Button label="New job" icon="pi pi-plus" @click="navigate" />
        </RouterLink>
      </div>
    </div>

    <Message v-if="jobTypes.loaded.value && !jobType" severity="warn">
      This job type is not known to the server, it might have been removed.
    </Message>

    <p v-if="jobType?.description" class="text-muted-color">{{ jobType.description }}</p>

    <div class="flex min-h-7 flex-wrap items-center gap-2">
      <span class="text-sm text-muted-color">Executors:</span>
      <RouterLink
        v-for="executor in jobTypeExecutors.slice(0, maxExecutors)"
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
        v-if="jobTypeExecutors.length > maxExecutors"
        :to="{ path: '/executors', query: { q: id } }"
        class="text-sm text-primary hover:underline"
      >
        +{{ jobTypeExecutors.length - maxExecutors }} more
      </RouterLink>
      <Skeleton v-if="!executors.loaded.value" width="6rem" height="1.5rem" />
      <Tag v-else-if="jobTypeExecutors.length === 0" value="None connected" severity="warn" />
    </div>

    <div class="flex min-h-7 flex-wrap items-center gap-x-6 gap-y-2">
      <div class="flex flex-wrap items-center gap-2">
        <span class="text-sm text-muted-color">Jobs:</span>
        <JobStatusCounts
          :counts="jobTypeCounts?.jobs"
          :failed="!!counts.state.error.value"
          :link="
            (status: ExecutionStatus) => jobsLink({ jobTypeIds: [id], executionStatuses: [status] })
          "
        />
      </div>
      <div class="flex items-center gap-2">
        <span class="text-sm text-muted-color">Active schedules:</span>
        <RouterLink
          v-if="jobTypeCounts"
          :to="schedulesLink({ jobTypeIds: [id], statuses: [ScheduleStatus.ACTIVE] })"
          class="text-sm text-primary tabular-nums hover:underline"
        >
          {{ formatCount(jobTypeCounts.activeSchedules) }}
        </RouterLink>
        <span v-else-if="counts.state.error.value" class="text-muted-color">–</span>
        <Skeleton v-else width="2rem" />
      </div>
      <LoadStatus :state="counts.state" verb="count" />
    </div>

    <div v-if="jobType" class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <Card>
        <template #title>Input schema</template>
        <template #content>
          <JsonEditor
            v-if="jobType.inputSchemaJson"
            :model-value="prettyJson(jobType.inputSchemaJson)"
            readonly
          />
          <span v-else class="text-muted-color">No schema, any JSON input is accepted.</span>
        </template>
      </Card>
      <Card>
        <template #title>Output schema</template>
        <template #content>
          <JsonEditor
            v-if="jobType.outputSchemaJson"
            :model-value="prettyJson(jobType.outputSchemaJson)"
            readonly
          />
          <span v-else class="text-muted-color">No schema.</span>
        </template>
      </Card>
    </div>
    <div v-else-if="!jobTypes.loaded.value" class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <Skeleton height="12rem" />
      <Skeleton height="12rem" />
    </div>

    <!-- Lazy, so that only the visible table is loaded. -->
    <Tabs v-model:value="tab" lazy>
      <TabList>
        <Tab value="jobs">Jobs</Tab>
        <Tab value="schedules">Schedules</Tab>
      </TabList>
      <TabPanels class="px-0!">
        <TabPanel value="jobs">
          <JobsTable
            ref="jobsTable"
            :base-filters="baseFilters"
            query-prefix="jobs."
            :refresh-interval="refresh"
          />
        </TabPanel>
        <TabPanel value="schedules">
          <SchedulesTable
            ref="schedulesTable"
            :base-filters="baseFilters"
            query-prefix="schedules."
            :refresh-interval="refresh"
          />
        </TabPanel>
      </TabPanels>
    </Tabs>
  </div>
</template>
