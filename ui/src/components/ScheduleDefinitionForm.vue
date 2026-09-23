<script setup lang="ts">
import { missedTimePolicyOptions } from "../util/schedule";
import type { ScheduleDraft } from "../util/scheduleDraft";

const draft = defineModel<ScheduleDraft>({ required: true });

const policyOptions = [
  { label: "Interval", value: "interval", icon: "pi pi-replay" },
  { label: "Cron", value: "cron", icon: "pi pi-calendar" },
];

const cronExamples = [
  { label: "Every minute", value: "* * * * *" },
  { label: "Every 5 minutes", value: "*/5 * * * *" },
  { label: "Hourly", value: "0 * * * *" },
  { label: "Daily at midnight UTC", value: "0 0 * * * UTC" },
  { label: "Mondays at 9:00", value: "0 9 * * 1" },
];
</script>

<template>
  <div class="flex flex-col gap-6">
    <Fieldset legend="Scheduling">
      <div class="flex flex-col gap-4">
        <div class="flex flex-col gap-1">
          <label class="font-medium">Schedule labels</label>
          <LabelsInput v-model="draft.labels" />
        </div>

        <SelectButton
          v-model="draft.policy"
          :options="policyOptions"
          option-label="label"
          option-value="value"
          :allow-empty="false"
        >
          <template #option="{ option }">
            <i :class="option.icon" />
            <span>{{ option.label }}</span>
          </template>
        </SelectButton>

        <div v-if="draft.policy === 'interval'" class="flex flex-col gap-1">
          <label for="interval" class="text-sm">Interval</label>
          <DurationInput
            v-model="draft.interval"
            input-id="interval"
            placeholder="Interval between jobs"
            class="max-w-md"
          />
        </div>

        <div v-else class="flex flex-col gap-1">
          <label for="cron" class="text-sm">Cron expression</label>
          <InputText
            id="cron"
            v-model="draft.cronExpression"
            placeholder="*/5 * * * *"
            :invalid="draft.cronExpression.trim() === ''"
            class="max-w-md font-mono"
          />
          <small class="text-muted-color">
            <span class="font-mono">minute hour day-of-month month day-of-week [timezone]</span>,
            the server's timezone is used if omitted.
            <a
              href="https://docs.rs/cronexpr/latest/cronexpr/"
              target="_blank"
              rel="noopener"
              class="text-primary hover:underline"
              >Syntax reference</a
            >
          </small>
          <div class="flex flex-wrap gap-1">
            <Button
              v-for="example in cronExamples"
              :key="example.value"
              v-tooltip.bottom="example.value"
              :label="example.label"
              severity="secondary"
              size="small"
              text
              @click="draft.cronExpression = example.value"
            />
          </div>
        </div>

        <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
          <label class="flex items-center gap-2">
            <ToggleSwitch v-model="draft.immediate" />
            <span>Create a job immediately</span>
          </label>
          <div class="flex flex-col gap-1">
            <label for="missed-time" class="text-sm">Missed times</label>
            <Select
              v-model="draft.missedTimePolicy"
              input-id="missed-time"
              :options="missedTimePolicyOptions"
              option-label="label"
              option-value="value"
              size="small"
            >
              <template #option="{ option }">
                <div class="flex flex-col">
                  <span>{{ option.label }}</span>
                  <span class="text-sm text-muted-color">{{ option.description }}</span>
                </div>
              </template>
            </Select>
          </div>
        </div>

        <div class="flex flex-col gap-1">
          <label class="text-sm">Active time range</label>
          <TimeRangeInput
            v-model="draft.timeRange"
            start-placeholder="From now"
            end-placeholder="Indefinitely"
          />
          <small class="text-muted-color">No jobs are created outside of this time range.</small>
        </div>
      </div>
    </Fieldset>

    <Fieldset legend="Job template">
      <JobDefinitionForm v-model="draft.job" template />
    </Fieldset>
  </div>
</template>
