<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { BackoffStrategy } from "../api/ora/jobs/v1/job_pb";
import { useExecutors, useJobTypes } from "../util/data";
import { prettyJson } from "../util/format";
import {
  backoffStrategyOptions,
  payloadTemplate,
  timeoutBaseTimeOptions,
  type JobDraft,
} from "../util/job";
import { parseSchema, validateJson } from "../util/schema";

const props = defineProps<{
  /** The job is a schedule template, the target time is decided by the schedule. */
  template?: boolean;
}>();

const draft = defineModel<JobDraft>({ required: true });

const { jobTypes, byId, loaded } = useJobTypes();
const { byJobType, loaded: executorsLoaded } = useExecutors();

const jobType = computed(() => byId.value.get(draft.value.jobTypeId));
const inputSchema = computed(() => parseSchema(jobType.value?.inputSchemaJson));
const validation = computed(() => validateJson(draft.value.payload, inputSchema.value));
const hasExecutor = computed(() => (byJobType.value.get(draft.value.jobTypeId)?.length ?? 0) > 0);

// Replace the payload with the schema template when the job type changes,
// unless the user has already edited it.
watch(
  jobType,
  (current, previous) => {
    const payload = draft.value.payload.trim();
    const untouched =
      payload === "" ||
      payload === "{}" ||
      (previous && payload === payloadTemplate(previous).trim());

    if (current && untouched) {
      draft.value.payload = payloadTemplate(current);
    }
  },
  { immediate: true },
);

function insertTemplate() {
  draft.value.payload = payloadTemplate(jobType.value);
}

function format() {
  draft.value.payload = prettyJson(draft.value.payload);
}

// Sections are expanded initially only if they are configured.
const expandedSections = ref([
  ...(draft.value.timeoutEnabled ? ["timeout"] : []),
  ...(draft.value.retries > 0 ? ["retry"] : []),
]);

const targetTimeOptions = [
  { label: "Now", value: "now" },
  { label: "At time", value: "at" },
];
</script>

<template>
  <div class="flex flex-col gap-5">
    <div class="flex flex-col gap-1">
      <label for="job-type" class="font-medium">Job type</label>
      <Select
        v-model="draft.jobTypeId"
        input-id="job-type"
        :options="jobTypes"
        option-label="id"
        option-value="id"
        placeholder="Select a job type"
        filter
        :loading="!loaded"
        fluid
      >
        <template #option="{ option }">
          <div class="flex flex-col">
            <span class="font-mono">{{ option.id }}</span>
            <span v-if="option.description" class="text-sm text-muted-color">
              {{ option.description }}
            </span>
          </div>
        </template>
      </Select>
      <small v-if="jobType?.description" class="text-muted-color">{{ jobType.description }}</small>
      <Message
        v-if="draft.jobTypeId && executorsLoaded && !hasExecutor"
        severity="warn"
        size="small"
        variant="simple"
      >
        No connected executor supports this job type, jobs will wait until one connects.
      </Message>
    </div>

    <div class="flex flex-col gap-1">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <label class="font-medium">Input payload</label>
        <div class="flex items-center gap-1">
          <RouterLink
            v-if="jobType"
            :to="`/job-types/${jobType.id}`"
            class="mr-2 text-sm text-primary hover:underline"
          >
            View schema
          </RouterLink>
          <Button
            label="Insert template"
            icon="pi pi-file"
            severity="secondary"
            text
            size="small"
            :disabled="!jobType"
            @click="insertTemplate"
          />
          <Button
            label="Format"
            icon="pi pi-align-left"
            severity="secondary"
            text
            size="small"
            @click="format"
          />
        </div>
      </div>
      <JsonEditor v-model="draft.payload" :schema="inputSchema" min-height="8rem" />
      <small class="text-muted-color">
        Validated against the job type's input schema, press
        <kbd class="font-mono">Ctrl+Space</kbd> for suggestions.
      </small>
      <Message
        v-for="(error, index) in validation.errors"
        :key="index"
        severity="error"
        size="small"
        variant="simple"
      >
        <span class="font-mono">{{ error.path }}</span
        >: {{ error.message }}
      </Message>
    </div>

    <div v-if="!props.template" class="flex flex-col gap-1">
      <label class="font-medium">Target execution time</label>
      <div class="flex flex-wrap items-center gap-2">
        <SelectButton
          v-model="draft.targetTimeMode"
          :options="targetTimeOptions"
          option-label="label"
          option-value="value"
          :allow-empty="false"
        />
        <DatePicker
          v-if="draft.targetTimeMode === 'at'"
          v-model="draft.targetTime"
          show-time
          hour-format="24"
          show-seconds
          show-icon
          :invalid="!draft.targetTime"
          placeholder="Select a time"
        />
      </div>
    </div>

    <div class="flex flex-col gap-1">
      <label class="font-medium">Labels</label>
      <LabelsInput v-model="draft.labels" />
    </div>

    <Accordion v-model:value="expandedSections" multiple>
      <AccordionPanel value="timeout">
        <AccordionHeader>
          <span class="flex items-center gap-2">
            Timeout policy
            <Tag v-if="draft.timeoutEnabled" value="Enabled" severity="info" />
          </span>
        </AccordionHeader>
        <AccordionContent>
          <div class="flex flex-col gap-3">
            <label class="flex items-center gap-2">
              <ToggleSwitch v-model="draft.timeoutEnabled" />
              <span>Time out executions</span>
            </label>
            <div v-if="draft.timeoutEnabled" class="grid grid-cols-1 gap-3 md:grid-cols-2">
              <div class="flex flex-col gap-1">
                <label for="timeout" class="text-sm">Timeout</label>
                <DurationInput v-model="draft.timeout" input-id="timeout" placeholder="Timeout" />
              </div>
              <div class="flex flex-col gap-1">
                <label for="timeout-base" class="text-sm">Measured from</label>
                <Select
                  v-model="draft.timeoutBaseTime"
                  input-id="timeout-base"
                  :options="timeoutBaseTimeOptions"
                  option-label="label"
                  option-value="value"
                  size="small"
                />
              </div>
            </div>
          </div>
        </AccordionContent>
      </AccordionPanel>

      <AccordionPanel value="retry">
        <AccordionHeader>
          <span class="flex items-center gap-2">
            Retry policy
            <Tag v-if="draft.retries > 0" :value="`${draft.retries} retries`" severity="info" />
          </span>
        </AccordionHeader>
        <AccordionContent>
          <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div class="flex flex-col gap-1">
              <label for="retries" class="text-sm">Retries</label>
              <InputNumber
                v-model="draft.retries"
                input-id="retries"
                :min="0"
                show-buttons
                size="small"
              />
              <small class="text-muted-color">Zero means failed executions are not retried.</small>
            </div>
            <div class="flex flex-col gap-1">
              <label for="backoff-strategy" class="text-sm">Backoff strategy</label>
              <Select
                v-model="draft.backoffStrategy"
                input-id="backoff-strategy"
                :options="backoffStrategyOptions"
                option-label="label"
                option-value="value"
                size="small"
              />
            </div>
            <div class="flex flex-col gap-1">
              <label for="backoff" class="text-sm">Backoff duration</label>
              <DurationInput v-model="draft.backoff" input-id="backoff" placeholder="Immediately" />
            </div>
            <div
              v-if="draft.backoffStrategy === BackoffStrategy.EXPONENTIAL"
              class="flex flex-col gap-1"
            >
              <label for="max-backoff" class="text-sm">Maximum backoff duration</label>
              <DurationInput
                v-model="draft.maxBackoff"
                input-id="max-backoff"
                placeholder="Unlimited"
              />
            </div>
          </div>
        </AccordionContent>
      </AccordionPanel>
    </Accordion>
  </div>
</template>
