<script setup lang="ts">
import { computed, ref } from "vue";
import { useExecutors } from "../../util/data";
import { executorLoad } from "../../util/executor";
import { formatClock, formatCount, formatRelative, formatTimestamp } from "../../util/format";
import { pageSizeOptions } from "../../util/pagination";
import { usePolling } from "../../util/polling";
import { param, useRouteQuery } from "../../util/query";
import { useRefreshInterval, useSearchQuery } from "../../util/route";

const executors = useExecutors();
const refresh = useRefreshInterval();
const search = useSearchQuery();
const rows = useRouteQuery("rows", param.int(pageSizeOptions), 20);

usePolling(executors, refresh);

/** Executors matching the search by name, ID or supported job type. */
const filtered = computed(() => {
  const query = search.value.trim().toLowerCase();
  if (!query) {
    return executors.executors.value;
  }

  return executors.executors.value.filter(
    executor =>
      executor.id.toLowerCase().includes(query) ||
      executor.name?.toLowerCase().includes(query) ||
      executor.queues.some(queue => queue.jobType?.id.toLowerCase().includes(query)),
  );
});

const expandedRows = ref<Record<string, boolean>>({});
</script>

<template>
  <div class="flex flex-col gap-4">
    <h1 class="text-2xl font-semibold">Executors</h1>

    <div class="relative">
      <LoadingBar :active="executors.busy.value" class="absolute inset-x-0 top-0 z-10" />
      <DataTable
        v-model:expanded-rows="expandedRows"
        :value="filtered"
        data-key="id"
        paginator
        v-model:rows="rows"
        :rows-per-page-options="pageSizeOptions"
      >
        <template #header>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex flex-wrap items-center gap-3">
              <IconField>
                <InputIcon class="pi pi-search" />
                <InputText
                  v-model="search"
                  placeholder="Search by name, ID or job type"
                  aria-label="Search executors"
                  class="w-80"
                />
              </IconField>
              <div class="min-w-32 text-sm text-muted-color tabular-nums">
                <Skeleton v-if="!executors.loaded.value" width="6rem" height="1.25rem" />
                <template v-else-if="search.trim()">
                  {{ formatCount(filtered.length) }} of
                  {{ formatCount(executors.executors.value.length) }} executor(s)
                </template>
                <template v-else>
                  {{ formatCount(filtered.length) }} connected executor(s)
                </template>
              </div>
            </div>
            <RefreshControl
              v-model="refresh"
              :loading="executors.busy.value"
              @refresh="executors.reload()"
            />
          </div>
        </template>
        <template #empty>
          <div v-if="!executors.loaded.value" class="flex flex-col gap-4 py-2" aria-busy="true">
            <Skeleton v-for="i in 5" :key="i" height="2rem" />
          </div>
          <div v-else class="py-6 text-center text-muted-color">
            {{ search.trim() ? "No executors match the search." : "No executors are connected." }}
          </div>
        </template>

        <template #paginatorstart>
          <div class="w-40 sm:w-64"><LoadStatus :state="executors.state" verb="list" /></div>
        </template>
        <template #paginatorend>
          <div class="w-40 text-right text-xs text-muted-color tabular-nums sm:w-64">
            <template v-if="executors.loadedAt.value !== undefined">
              Updated {{ formatClock(executors.loadedAt.value) }}
            </template>
          </div>
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
            <span v-tooltip.top="formatTimestamp(data.lastSeenAt)" class="whitespace-nowrap">{{
              formatRelative(data.lastSeenAt)
            }}</span>
          </template>
        </Column>
        <Column header="Job types">
          <template #body="{ data }">
            <span class="tabular-nums">{{ data.queues.length }}</span>
          </template>
        </Column>
        <Column header="Active executions" class="w-64">
          <template #body="{ data }">
            <div class="flex items-center gap-2">
              <ProgressBar
                :value="executorLoad(data).percent"
                :show-value="false"
                class="h-2! flex-1"
              />
              <span class="text-sm whitespace-nowrap tabular-nums">
                {{ executorLoad(data).active }} / {{ executorLoad(data).max }}
              </span>
            </div>
          </template>
        </Column>

        <template #expansion="{ data }">
          <DataTable
            :value="data.queues"
            size="small"
            :paginator="data.queues.length > 10"
            :rows="10"
          >
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
                  <span class="text-sm whitespace-nowrap tabular-nums">
                    {{ queue.activeExecutions }} / {{ queue.maxConcurrentExecutions }}
                  </span>
                </div>
              </template>
            </Column>
          </DataTable>
        </template>
      </DataTable>
    </div>
  </div>
</template>
