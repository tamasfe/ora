<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useConfirm } from "primevue/useconfirm";
import { useToast } from "primevue/usetoast";
import { timestampMs } from "@bufbuild/protobuf/wkt";
import { ExecutionStatus, type Execution } from "../../api/ora/admin/v1/executions_pb";
import type { Label } from "../../api/ora/common/v1/label_pb";
import { useOraAdminClient } from "../../grpc";
import { useLoader } from "../../util";
import { useJobTypes } from "../../util/data";
import { useErrorToast } from "../../util/errors";
import {
  formatDuration,
  formatLatency,
  formatMs,
  formatRelative,
  formatTimestamp,
  prettyJson,
} from "../../util/format";
import { backoffStrategyLabel, hasTimeout, timeoutBaseTimeLabel } from "../../util/job";
import { labelsToFilters } from "../../util/labels";
import { usePolling } from "../../util/polling";
import { jobsLink, useRefreshInterval } from "../../util/route";
import { parseSchema, validateJson } from "../../util/schema";
import {
  executionEndedAt,
  executionStatusInfo,
  isJobActive,
  jobEndedAt,
  jobStartedAt,
  jobStatus,
} from "../../util/status";
import { useStopwatch } from "../../util/time";

const route = useRoute("/jobs/[id]");
const router = useRouter();
const client = useOraAdminClient();
const confirm = useConfirm();
const toast = useToast();
const reportError = useErrorToast();
const { byId } = useJobTypes();

// Reading the route in the loader would reload on any query change.
const id = computed(() => route.params.id);
const refresh = useRefreshInterval();

const loader = useLoader(
  () => id.value,
  async (jobId, signal) => {
    const res = await client.listJobs(
      { filters: { jobIds: [jobId] }, pagination: { pageSize: 1 } },
      { signal },
    );
    return res.jobs[0] ?? null;
  },
);

usePolling(loader, refresh);

/** The job, not shown while another job is loading. */
const job = computed(() => (loader.stale.value ? undefined : (loader.data.value ?? undefined)));
const notFound = computed(() => !loader.stale.value && loader.data.value === null);
const definition = computed(() => job.value?.job);
const jobType = computed(() => byId.value.get(definition.value?.jobTypeId ?? ""));
const status = computed(() => (job.value ? executionStatusInfo[jobStatus(job.value)] : undefined));
const outputSchema = computed(() => parseSchema(jobType.value?.outputSchemaJson));
const active = computed(() => !!job.value && isJobActive(job.value));
const startedAt = computed(() => (job.value ? jobStartedAt(job.value) : undefined));
const endedAt = computed(() => (job.value ? jobEndedAt(job.value) : undefined));

/** Executions in chronological order with their attempt numbers. */
const executions = computed(() =>
  [...(job.value?.executions ?? [])]
    .sort((a, b) =>
      a.createdAt && b.createdAt ? timestampMs(a.createdAt) - timestampMs(b.createdAt) : 0,
    )
    .map((execution, index) => ({ execution, attempt: index + 1 }))
    .reverse(),
);

const executionRows = 10;
const expandedRows = ref<Record<string, boolean>>({});

// Expand the latest execution by default.
watch(
  () => executions.value[0]?.execution.id,
  latest => {
    if (latest && Object.keys(expandedRows.value).length === 0) {
      expandedRows.value = { [latest]: true };
    }
  },
);

function outputValidation(execution: Execution) {
  if (execution.outputJson === undefined || !outputSchema.value) {
    return undefined;
  }
  return validateJson(execution.outputJson, outputSchema.value);
}

/** The time between the target time and the start of an execution. */
function startDelay(execution: Execution) {
  if (!execution.startedAt || !execution.targetExecutionTime) {
    return "-";
  }
  return formatMs(
    Math.max(0, timestampMs(execution.startedAt) - timestampMs(execution.targetExecutionTime)),
  );
}

function labelLink(label: Label) {
  return jobsLink({ labels: [{ key: label.key, value: label.value }] });
}

const cancelling = useStopwatch();

function cancel() {
  confirm.require({
    header: "Cancel job",
    message: "Cancel this job? Running executions will be cancelled as well.",
    icon: "pi pi-exclamation-triangle",
    rejectProps: { label: "Keep", severity: "secondary", outlined: true },
    acceptProps: { label: "Cancel job", severity: "danger" },
    accept: async () => {
      try {
        const { ms } = await cancelling.time(() =>
          client.cancelJobs({ filters: { jobIds: [id.value] } }),
        );
        toast.add({
          severity: "success",
          summary: "Job cancelled",
          detail: `Cancelled in ${formatLatency(ms)}.`,
          life: 5000,
        });
        loader.reload(true);
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
        <CopyableId :id="id" class="text-muted-color" />
        <Tag v-if="status" :value="status.label" :severity="status.severity" :icon="status.icon" />
        <Skeleton v-else-if="!notFound" width="6rem" height="1.75rem" />
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <LoadStatus :state="loader" verb="load" updated class="min-w-56 text-right" />
        <RefreshControl v-model="refresh" :loading="loader.busy.value" @refresh="loader.reload()" />
        <RouterLink v-slot="{ navigate }" :to="{ path: '/jobs/new', query: { from: id } }" custom>
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
          label="Cancel"
          icon="pi pi-ban"
          severity="danger"
          :disabled="!active || cancelling.running.value"
          :loading="cancelling.running.value"
          @click="cancel"
        />
      </div>
    </div>

    <Message v-if="notFound" severity="warn">Job not found.</Message>

    <template v-else>
      <div class="flex min-h-7 flex-wrap items-center gap-2">
        <span class="text-sm text-muted-color"><i class="pi pi-tags mr-1" />Labels</span>
        <template v-if="definition">
          <LabelList
            v-if="definition.labels.length > 0"
            :labels="definition.labels"
            size="normal"
            :link="labelLink"
            hint="Show jobs with this label"
          />
          <span v-else class="text-sm text-muted-color">None</span>
          <RouterLink
            v-if="definition.labels.length > 1"
            :to="jobsLink({ labels: labelsToFilters(definition.labels) })"
            class="text-sm text-primary hover:underline"
          >
            Jobs with the same labels
          </RouterLink>
        </template>
        <Skeleton v-else width="16rem" height="1.5rem" />
      </div>

      <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card>
          <template #title>Details</template>
          <template #content>
            <dl
              v-if="job && definition"
              class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm"
            >
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
              <dt class="text-muted-color">Started</dt>
              <dd>
                <template v-if="startedAt">
                  {{ formatTimestamp(startedAt) }}
                  <span class="text-muted-color">({{ formatRelative(startedAt) }})</span>
                </template>
                <span v-else>-</span>
              </dd>
              <dt class="text-muted-color">Finished</dt>
              <dd>
                <template v-if="endedAt">
                  {{ formatTimestamp(endedAt) }}
                  <span class="text-muted-color">({{ formatRelative(endedAt) }})</span>
                </template>
                <span v-else>-</span>
              </dd>
              <dt class="text-muted-color">Duration</dt>
              <dd>
                <ElapsedTime :start="startedAt" :end="endedAt" :live="active" />
                <span v-if="executions.length > 1" class="text-muted-color">
                  (over {{ executions.length }} attempts)
                </span>
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
            </dl>
            <div v-else class="flex flex-col gap-3">
              <Skeleton v-for="i in 9" :key="i" height="1.25rem" />
            </div>
          </template>
        </Card>

        <Card>
          <template #title>Input payload</template>
          <template #content>
            <JsonEditor
              v-if="definition"
              :model-value="prettyJson(definition.inputPayloadJson)"
              readonly
            />
            <Skeleton v-else height="8rem" />
          </template>
        </Card>
      </div>

      <Card>
        <template #title>
          Executions
          <span v-if="job" class="text-base font-normal text-muted-color tabular-nums"
            >({{ executions.length }})</span
          >
        </template>
        <template #content>
          <DataTable
            v-if="job"
            v-model:expanded-rows="expandedRows"
            :value="executions"
            data-key="execution.id"
            size="small"
            :paginator="executions.length > executionRows"
            :rows="executionRows"
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
              <template #body="{ data }">
                <span class="tabular-nums">{{
                  formatTimestamp(data.execution.targetExecutionTime)
                }}</span>
              </template>
            </Column>
            <Column header="Started">
              <template #body="{ data }">
                <span class="tabular-nums">{{ formatTimestamp(data.execution.startedAt) }}</span>
              </template>
            </Column>
            <Column header="Ended">
              <template #body="{ data }">
                <span class="tabular-nums">{{
                  formatTimestamp(executionEndedAt(data.execution))
                }}</span>
              </template>
            </Column>
            <Column header="Duration">
              <template #body="{ data }">
                <ElapsedTime
                  :start="data.execution.startedAt"
                  :end="executionEndedAt(data.execution)"
                  :live="data.execution.status === ExecutionStatus.IN_PROGRESS"
                />
              </template>
            </Column>

            <template #expansion="{ data }">
              <div class="flex flex-col gap-3 p-2">
                <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1 text-sm">
                  <dt class="text-muted-color">Execution ID</dt>
                  <dd><CopyableId :id="data.execution.id" /></dd>
                  <dt class="text-muted-color">Created</dt>
                  <dd>{{ formatTimestamp(data.execution.createdAt) }}</dd>
                  <dt class="text-muted-color">Start delay</dt>
                  <dd v-tooltip.top="'Time between the target time and the start'" class="w-fit">
                    {{ startDelay(data.execution) }}
                  </dd>
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
          <div v-else class="flex flex-col gap-3">
            <Skeleton v-for="i in 3" :key="i" height="2rem" />
          </div>
        </template>
      </Card>
    </template>
  </div>
</template>
