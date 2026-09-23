<script setup lang="ts">
import { ref } from "vue";
import { useExecutors } from "../../util/data";
import { executorLoad } from "../../util/executor";
import { formatRelative, formatTimestamp } from "../../util/format";

const { executors, loading, reload } = useExecutors();

const expandedRows = ref<Record<string, boolean>>({});
</script>

<template>
  <div class="flex flex-col gap-4">
    <h1 class="text-2xl font-semibold">Executors</h1>

    <DataTable
      v-model:expanded-rows="expandedRows"
      :value="executors"
      data-key="id"
      :loading="loading"
    >
      <template #header>
        <div class="flex items-center justify-between gap-2">
          <span class="text-sm text-muted-color">{{ executors.length }} connected executor(s)</span>
          <RefreshControl :loading="loading" @refresh="reload" />
        </div>
      </template>
      <template #empty>
        <div class="py-6 text-center text-muted-color">No executors are connected.</div>
      </template>

      <Column expander header-style="width: 3rem" />
      <Column header="Name">
        <template #body="{ data }">
          <RouterLink :to="`/executors/${data.id}`" class="text-primary hover:underline">
            {{ data.name ?? "(unnamed)" }}
          </RouterLink>
        </template>
      </Column>
      <Column header="ID">
        <template #body="{ data }">
          <CopyableId :id="data.id" />
        </template>
      </Column>
      <Column header="Last seen">
        <template #body="{ data }">
          <span v-tooltip.top="formatTimestamp(data.lastSeenAt)">{{
            formatRelative(data.lastSeenAt)
          }}</span>
        </template>
      </Column>
      <Column header="Job types">
        <template #body="{ data }">{{ data.queues.length }}</template>
      </Column>
      <Column header="Active executions" class="w-64">
        <template #body="{ data }">
          <div class="flex items-center gap-2">
            <ProgressBar
              :value="executorLoad(data).percent"
              :show-value="false"
              class="h-2! flex-1"
            />
            <span class="text-sm whitespace-nowrap">
              {{ executorLoad(data).active }} / {{ executorLoad(data).max }}
            </span>
          </div>
        </template>
      </Column>

      <template #expansion="{ data }">
        <DataTable :value="data.queues" size="small">
          <Column header="Job type">
            <template #body="{ data: queue }">
              <RouterLink
                :to="`/job-types/${queue.jobType?.id}`"
                class="font-mono text-primary hover:underline"
              >
                {{ queue.jobType?.id }}
              </RouterLink>
            </template>
          </Column>
          <Column header="Description">
            <template #body="{ data: queue }">{{ queue.jobType?.description }}</template>
          </Column>
          <Column header="Active / max concurrent" class="w-64">
            <template #body="{ data: queue }">
              <div class="flex items-center gap-2">
                <ProgressBar
                  :value="
                    Math.round(
                      (Number(queue.activeExecutions) /
                        Math.max(Number(queue.maxConcurrentExecutions), 1)) *
                        100,
                    )
                  "
                  :show-value="false"
                  class="h-2! flex-1"
                />
                <span class="text-sm whitespace-nowrap">
                  {{ queue.activeExecutions }} / {{ queue.maxConcurrentExecutions }}
                </span>
              </div>
            </template>
          </Column>
        </DataTable>
      </template>
    </DataTable>
  </div>
</template>
