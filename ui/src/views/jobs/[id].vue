<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { useToast } from "primevue/usetoast";
import { timestampMs } from "@bufbuild/protobuf/wkt";
import type { Execution } from "../../api/ora/admin/v1/executions_pb";
import { useOraAdminClient } from "../../grpc";
import { useLoader } from "../../util";
import { useJobTypes } from "../../util/data";
import { useErrorToast } from "../../util/errors";
import {
  formatDuration,
  formatElapsed,
  formatRelative,
  formatTimestamp,
  prettyJson,
} from "../../util/format";
import { backoffStrategyLabel, hasTimeout, timeoutBaseTimeLabel } from "../../util/job";
import { parseSchema, validateJson } from "../../util/schema";
import { executionEndedAt, executionStatusInfo, isJobActive, jobStatus } from "../../util/status";

const route = useRoute("/jobs/[id]");
const router = useRouter();
const client = useOraAdminClient();
const confirm = useConfirm();
const toast = useToast();
const reportError = useErrorToast();
const { byId } = useJobTypes();

const loader = useLoader(async signal => {
  const res = await client.listJobs(
    { filters: { jobIds: [route.params.id] }, pagination: { pageSize: 1 } },
    { signal },
  );
  return res.jobs[0] ?? null;
});

const job = computed(() => loader.data.value);
const definition = computed(() => job.value?.job);
const jobType = computed(() => byId.value.get(definition.value?.jobTypeId ?? ""));
const status = computed(() => (job.value ? executionStatusInfo[jobStatus(job.value)] : undefined));
const outputSchema = computed(() => parseSchema(jobType.value?.outputSchemaJson));

/** Executions in chronological order with their attempt numbers. */
const executions = computed(() =>
  [...(job.value?.executions ?? [])]
    .sort((a, b) =>
      a.createdAt && b.createdAt ? timestampMs(a.createdAt) - timestampMs(b.createdAt) : 0,
    )
    .map((execution, index) => ({ execution, attempt: index + 1 }))
    .reverse(),
);

const expandedRows = ref<Record<string, boolean>>({});

// Expand the latest execution by default.
watch(
  () => executions.value[0]?.execution.id,
  id => {
    if (id && Object.keys(expandedRows.value).length === 0) {
      expandedRows.value = { [id]: true };
    }
  },
);

function outputValidation(execution: Execution) {
  if (execution.outputJson === undefined || !outputSchema.value) {
    return undefined;
  }
  return validateJson(execution.outputJson, outputSchema.value);
}

function cancel() {
  confirm.require({
    header: "Cancel job",
    message: "Cancel this job? Running executions will be cancelled as well.",
    icon: "pi pi-exclamation-triangle",
    rejectProps: { label: "Keep", severity: "secondary", outlined: true },
    acceptProps: { label: "Cancel job", severity: "danger" },
    accept: async () => {
      try {
        await client.cancelJobs({ filters: { jobIds: [route.params.id] } });
        toast.add({ severity: "success", summary: "Job cancelled", life: 5000 });
        loader.reload();
      } catch (error) {
        reportError(error, "Failed to cancel job");
      }
    },
  });
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
        <h1 class="text-2xl font-semibold">Job</h1>
        <CopyableId :id="route.params.id" class="text-muted-color" />
        <Tag v-if="status" :value="status.label" :severity="status.severity" :icon="status.icon" />
      </div>
      <div class="flex items-center gap-2">
        <RefreshControl :loading="loader.loading.value" @refresh="loader.reload" />
        <RouterLink
          v-slot="{ navigate }"
          :to="{ path: '/jobs/new', query: { from: route.params.id } }"
          custom
        >
          <Button
            label="Clone"
            icon="pi pi-clone"
            severity="secondary"
            outlined
            :disabled="!job"
            @click="navigate"
          />
        </RouterLink>
        <Button
          v-if="job && isJobActive(job)"
          label="Cancel"
          icon="pi pi-ban"
          severity="danger"
          @click="cancel"
        />
      </div>
    </div>

    <Message v-if="loader.data.value === null" severity="warn">Job not found.</Message>
    <ProgressBar v-else-if="!job && loader.loading.value" mode="indeterminate" class="h-1!" />

    <template v-if="job && definition">
      <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card>
          <template #title>Details</template>
          <template #content>
            <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
              <dt class="text-muted-color">Job type</dt>
              <dd>
                <RouterLink
                  :to="`/job-types/${definition.jobTypeId}`"
                  class="font-mono text-primary hover:underline"
                >
                  {{ definition.jobTypeId }}
                </RouterLink>
              </dd>
              <dt class="text-muted-color">Target time</dt>
              <dd>
                {{ formatTimestamp(definition.targetExecutionTime) }}
                <span class="text-muted-color"
                  >({{ formatRelative(definition.targetExecutionTime) }})</span
                >
              </dd>
              <dt class="text-muted-color">Created</dt>
              <dd>
                {{ formatTimestamp(job.createdAt) }}
                <span class="text-muted-color">({{ formatRelative(job.createdAt) }})</span>
              </dd>
              <dt class="text-muted-color">Schedule</dt>
              <dd>
                <CopyableId
                  v-if="job.scheduleId"
                  :id="job.scheduleId"
                  :to="`/schedules/${job.scheduleId}`"
                />
                <span v-else>-</span>
              </dd>
              <dt class="text-muted-color">Timeout</dt>
              <dd>
                <template v-if="hasTimeout(definition.timeoutPolicy)">
                  {{ formatDuration(definition.timeoutPolicy?.timeout) }}
                  <span class="text-muted-color">
                    (from
                    {{ timeoutBaseTimeLabel(definition.timeoutPolicy!.baseTime).toLowerCase() }})
                  </span>
                </template>
                <span v-else>None</span>
              </dd>
              <dt class="text-muted-color">Retries</dt>
              <dd>
                {{ definition.retryPolicy?.retries ?? 0 }}
                <span
                  v-if="definition.retryPolicy && definition.retryPolicy.retries > 0n"
                  class="text-muted-color"
                >
                  ({{
                    backoffStrategyLabel(definition.retryPolicy.backoffStrategy).toLowerCase()
                  }}
                  backoff {{ formatDuration(definition.retryPolicy.backoffDuration)
                  }}<template v-if="definition.retryPolicy.maxBackoffDuration"
                    >, max {{ formatDuration(definition.retryPolicy.maxBackoffDuration) }}</template
                  >)
                </span>
              </dd>
              <dt class="text-muted-color">Labels</dt>
              <dd class="flex flex-wrap gap-1">
                <Tag
                  v-for="label in definition.labels"
                  :key="label.key"
                  :value="`${label.key}=${label.value}`"
                  severity="secondary"
                  class="font-normal!"
                />
                <span v-if="definition.labels.length === 0">-</span>
              </dd>
            </dl>
          </template>
        </Card>

        <Card>
          <template #title>Input payload</template>
          <template #content>
            <JsonEditor :model-value="prettyJson(definition.inputPayloadJson)" readonly />
          </template>
        </Card>
      </div>

      <Card>
        <template #title>Executions</template>
        <template #content>
          <DataTable
            v-model:expanded-rows="expandedRows"
            :value="executions"
            data-key="execution.id"
            size="small"
          >
            <template #empty>
              <div class="py-4 text-center text-muted-color">No executions yet.</div>
            </template>
            <Column expander header-style="width: 3rem" />
            <Column header="#" field="attempt" />
            <Column header="ID">
              <template #body="{ data }">
                <CopyableId :id="data.execution.id" short />
              </template>
            </Column>
            <Column header="Status">
              <template #body="{ data }">
                <Tag
                  :value="
                    executionStatusInfo[data.execution.status as keyof typeof executionStatusInfo]
                      .label
                  "
                  :severity="
                    executionStatusInfo[data.execution.status as keyof typeof executionStatusInfo]
                      .severity
                  "
                  :icon="
                    executionStatusInfo[data.execution.status as keyof typeof executionStatusInfo]
                      .icon
                  "
                />
              </template>
            </Column>
            <Column header="Executor">
              <template #body="{ data }">
                <CopyableId
                  v-if="data.execution.executorId"
                  :id="data.execution.executorId"
                  short
                  :to="`/executors/${data.execution.executorId}`"
                />
                <span v-else class="text-muted-color">-</span>
              </template>
            </Column>
            <Column header="Target time">
              <template #body="{ data }">{{
                formatTimestamp(data.execution.targetExecutionTime)
              }}</template>
            </Column>
            <Column header="Started">
              <template #body="{ data }">{{ formatTimestamp(data.execution.startedAt) }}</template>
            </Column>
            <Column header="Ended">
              <template #body="{ data }">{{
                formatTimestamp(executionEndedAt(data.execution))
              }}</template>
            </Column>
            <Column header="Duration">
              <template #body="{ data }">
                {{ formatElapsed(data.execution.startedAt, executionEndedAt(data.execution)) }}
              </template>
            </Column>

            <template #expansion="{ data }">
              <div class="flex flex-col gap-3 p-2">
                <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1 text-sm">
                  <dt class="text-muted-color">Execution ID</dt>
                  <dd><CopyableId :id="data.execution.id" /></dd>
                  <dt class="text-muted-color">Created</dt>
                  <dd>{{ formatTimestamp(data.execution.createdAt) }}</dd>
                </dl>
                <Message v-if="data.execution.failureReason" severity="error" :closable="false">
                  <pre class="font-mono text-sm whitespace-pre-wrap">{{
                    data.execution.failureReason
                  }}</pre>
                </Message>
                <div v-if="data.execution.outputJson !== undefined" class="flex flex-col gap-1">
                  <div class="flex items-center gap-2">
                    <span class="text-sm font-medium">Output</span>
                    <Tag
                      v-if="outputValidation(data.execution)?.valid === true"
                      value="Matches output schema"
                      severity="success"
                      icon="pi pi-check"
                    />
                    <Tag
                      v-else-if="outputValidation(data.execution)?.valid === false"
                      v-tooltip.top="
                        outputValidation(data.execution)
                          ?.errors.map(e => `${e.path}: ${e.message}`)
                          .join('\n')
                      "
                      value="Does not match output schema"
                      severity="warn"
                      icon="pi pi-exclamation-triangle"
                    />
                  </div>
                  <JsonEditor
                    :model-value="prettyJson(data.execution.outputJson)"
                    readonly
                    min-height="2rem"
                  />
                </div>
                <span
                  v-if="!data.execution.failureReason && data.execution.outputJson === undefined"
                  class="text-sm text-muted-color"
                >
                  No output yet.
                </span>
              </div>
            </template>
          </DataTable>
        </template>
      </Card>
    </template>
  </div>
</template>
