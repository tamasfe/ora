<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useExecutors } from "../../util/data";
import { executorLoad } from "../../util/executor";
import { formatRelative, formatTimestamp } from "../../util/format";
import { usePolling } from "../../util/polling";
import { useRefreshInterval } from "../../util/route";

const route = useRoute("/executors/[id]");
const router = useRouter();

const id = computed(() => route.params.id);
const refresh = useRefreshInterval();
const executors = useExecutors();

usePolling(executors, refresh);

const executor = computed(() => executors.executors.value.find(e => e.id === id.value));
const jobsFilters = computed(() => ({ executorIds: [id.value] }));

const jobsTable = ref<{ reload(): void }>();

function reload() {
  executors.reload();
  jobsTable.value?.reload();
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
        <h1 class="truncate text-2xl font-semibold">{{ executor?.name ?? "Executor" }}</h1>
        <CopyableId :id="id" class="text-muted-color" />
        <Tag v-if="executor" value="Connected" severity="success" icon="pi pi-circle-fill" />
        <Tag v-else-if="executors.loaded.value" value="Not connected" severity="secondary" />
        <Skeleton v-else width="6rem" height="1.75rem" />
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <LoadStatus :state="executors.state" verb="load" updated class="min-w-56 text-right" />
        <RefreshControl v-model="refresh" :loading="executors.busy.value" @refresh="reload" />
      </div>
    </div>

    <Message v-if="executors.loaded.value && !executor" severity="info">
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
        <DataTable
          :value="executor.queues"
          size="small"
          :paginator="executor.queues.length > 10"
          :rows="10"
        >
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
                <span class="text-sm whitespace-nowrap tabular-nums">
                  {{ data.activeExecutions }} / {{ data.maxConcurrentExecutions }}
                </span>
              </div>
            </template>
          </Column>
        </DataTable>
      </template>
    </Card>
    <Card v-else-if="!executors.loaded.value">
      <template #title>Queues</template>
      <template #content>
        <div class="flex flex-col gap-3">
          <Skeleton v-for="i in 3" :key="i" height="2rem" />
        </div>
      </template>
    </Card>

    <div class="flex flex-col gap-2">
      <h2 class="text-xl font-semibold">Jobs</h2>
      <JobsTable
        ref="jobsTable"
        :base-filters="jobsFilters"
        query-prefix="jobs."
        :refresh-interval="refresh"
      />
    </div>
  </div>
</template>
