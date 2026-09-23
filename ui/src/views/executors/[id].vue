<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useExecutors } from "../../util/data";
import { executorLoad } from "../../util/executor";
import { formatRelative, formatTimestamp } from "../../util/format";

const route = useRoute("/executors/[id]");
const router = useRouter();

const { executors, loading, loaded, reload } = useExecutors();

const executor = computed(() => executors.value.find(e => e.id === route.params.id));
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
        <h1 class="text-2xl font-semibold">{{ executor?.name ?? "Executor" }}</h1>
        <CopyableId :id="route.params.id" class="text-muted-color" />
        <Tag v-if="executor" value="Connected" severity="success" icon="pi pi-circle-fill" />
        <Tag v-else-if="loaded" value="Not connected" severity="secondary" />
      </div>
      <RefreshControl :loading="loading" @refresh="reload" />
    </div>

    <Message v-if="loaded && !executor" severity="info">
      This executor is not connected, only its past jobs are shown.
    </Message>

    <Card v-if="executor">
      <template #title>Queues</template>
      <template #subtitle>
        Last seen {{ formatRelative(executor.lastSeenAt) }} ({{
          formatTimestamp(executor.lastSeenAt)
        }}), {{ executorLoad(executor).active }} / {{ executorLoad(executor).max }} active
        executions
      </template>
      <template #content>
        <DataTable :value="executor.queues" size="small">
          <Column header="Job type">
            <template #body="{ data }">
              <RouterLink
                :to="`/job-types/${data.jobType?.id}`"
                class="font-mono text-primary hover:underline"
              >
                {{ data.jobType?.id }}
              </RouterLink>
            </template>
          </Column>
          <Column header="Description">
            <template #body="{ data }">{{ data.jobType?.description }}</template>
          </Column>
          <Column header="Active / max concurrent" class="w-64">
            <template #body="{ data }">
              <div class="flex items-center gap-2">
                <ProgressBar
                  :value="
                    Math.round(
                      (Number(data.activeExecutions) /
                        Math.max(Number(data.maxConcurrentExecutions), 1)) *
                        100,
                    )
                  "
                  :show-value="false"
                  class="h-2! flex-1"
                />
                <span class="text-sm whitespace-nowrap">
                  {{ data.activeExecutions }} / {{ data.maxConcurrentExecutions }}
                </span>
              </div>
            </template>
          </Column>
        </DataTable>
      </template>
    </Card>

    <div class="flex flex-col gap-2">
      <h2 class="text-xl font-semibold">Jobs</h2>
      <JobsTable :base-filters="{ executorIds: [route.params.id] }" />
    </div>
  </div>
</template>
