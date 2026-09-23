<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useExecutors, useJobTypes } from "../../util/data";
import { prettyJson, shortId } from "../../util/format";

const route = useRoute("/job-types/[id]");
const router = useRouter();

const { byId, loaded } = useJobTypes();
const { byJobType } = useExecutors();

const jobType = computed(() => byId.value.get(route.params.id));
const executors = computed(() => byJobType.value.get(route.params.id) ?? []);
const baseFilters = computed(() => ({ jobTypeIds: [route.params.id] }));

const tab = ref("jobs");
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
        <h1 class="truncate font-mono text-2xl font-semibold">{{ route.params.id }}</h1>
      </div>
      <div class="flex items-center gap-2">
        <RouterLink
          v-slot="{ navigate }"
          :to="{ path: '/schedules/new', query: { jobType: route.params.id } }"
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
          :to="{ path: '/jobs/new', query: { jobType: route.params.id } }"
          custom
        >
          <Button label="New job" icon="pi pi-plus" @click="navigate" />
        </RouterLink>
      </div>
    </div>

    <Message v-if="loaded && !jobType" severity="warn">
      This job type is not known to the server, it might have been removed.
    </Message>

    <p v-if="jobType?.description" class="text-muted-color">{{ jobType.description }}</p>

    <div class="flex flex-wrap items-center gap-2">
      <span class="text-sm text-muted-color">Executors:</span>
      <RouterLink
        v-for="executor in executors"
        :key="executor.id"
        :to="`/executors/${executor.id}`"
      >
        <Tag
          :value="executor.name ?? shortId(executor.id)"
          severity="secondary"
          class="font-normal!"
        />
      </RouterLink>
      <Tag v-if="executors.length === 0" value="None connected" severity="warn" />
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

    <Tabs v-model:value="tab">
      <TabList>
        <Tab value="jobs">Jobs</Tab>
        <Tab value="schedules">Schedules</Tab>
      </TabList>
      <TabPanels class="px-0!">
        <TabPanel value="jobs">
          <JobsTable :base-filters="baseFilters" />
        </TabPanel>
        <TabPanel value="schedules">
          <SchedulesTable :base-filters="baseFilters" />
        </TabPanel>
      </TabPanels>
    </Tabs>
  </div>
</template>
