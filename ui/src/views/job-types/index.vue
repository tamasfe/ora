<script setup lang="ts">
import { useExecutors, useJobTypes } from "../../util/data";
import { shortId } from "../../util/format";
import { jobsLink, schedulesLink } from "../../util/route";

const { jobTypes, loading, reload } = useJobTypes();
const { byJobType } = useExecutors();

// Job types appear as executors connect, always show the latest list here.
reload();
</script>

<template>
  <div class="flex flex-col gap-4">
    <h1 class="text-2xl font-semibold">Job Types</h1>

    <DataTable :value="jobTypes" data-key="id" :loading="loading">
      <template #header>
        <div class="flex items-center justify-between gap-2">
          <span class="text-sm text-muted-color">{{ jobTypes.length }} job type(s)</span>
          <RefreshControl :loading="loading" @refresh="reload" />
        </div>
      </template>
      <template #empty>
        <div class="py-6 text-center text-muted-color">No job types are known to the server.</div>
      </template>

      <Column header="ID">
        <template #body="{ data }">
          <RouterLink :to="`/job-types/${data.id}`" class="font-mono text-primary hover:underline">
            {{ data.id }}
          </RouterLink>
        </template>
      </Column>
      <Column header="Description" field="description" />
      <Column header="Executors">
        <template #body="{ data }">
          <div class="flex flex-wrap gap-1">
            <RouterLink
              v-for="executor in byJobType.get(data.id)"
              :key="executor.id"
              :to="`/executors/${executor.id}`"
            >
              <Tag
                :value="executor.name ?? shortId(executor.id)"
                severity="secondary"
                class="font-normal!"
              />
            </RouterLink>
            <Tag v-if="!byJobType.get(data.id)" value="None connected" severity="warn" />
          </div>
        </template>
      </Column>
      <Column header-class="w-0">
        <template #body="{ data }">
          <div class="flex justify-end gap-1 whitespace-nowrap">
            <RouterLink v-slot="{ navigate }" :to="jobsLink({ jobTypeIds: [data.id] })" custom>
              <Button label="Jobs" severity="secondary" text size="small" @click="navigate" />
            </RouterLink>
            <RouterLink v-slot="{ navigate }" :to="schedulesLink({ jobTypeIds: [data.id] })" custom>
              <Button label="Schedules" severity="secondary" text size="small" @click="navigate" />
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
</template>
