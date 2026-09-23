<script setup lang="ts">
import { computed, ref } from "vue";
import { create } from "@bufbuild/protobuf";
import { JobFiltersSchema, type JobFilters } from "../api/ora/admin/v1/jobs_pb";
import { useJobTypes } from "../util/data";
import { executionStatusOptions } from "../util/status";

const props = defineProps<{
  /** Filters that are not editable (e.g. fixed by the parent view). */
  hidden?: (keyof JobFilters)[];
}>();

const model = defineModel<JobFilters>({ required: true });

const { jobTypes, byId, loaded } = useJobTypes();

function visible(key: keyof JobFilters) {
  return !props.hidden?.includes(key);
}

function set<K extends keyof JobFilters>(key: K, value: JobFilters[K]) {
  model.value = { ...model.value, [key]: value };
}

const idFilters = [
  { key: "jobIds", label: "Job IDs" },
  { key: "scheduleIds", label: "Schedule IDs" },
  { key: "executorIds", label: "Executor IDs" },
  { key: "executionIds", label: "Execution IDs" },
] as const;

function isSet(key: keyof JobFilters): boolean {
  const value = model.value[key];
  return visible(key) && (Array.isArray(value) ? value.length > 0 : value !== undefined);
}

/** Filters in the collapsible section. */
const moreKeys = [
  "jobIds",
  "scheduleIds",
  "executorIds",
  "executionIds",
  "targetExecutionTime",
  "createdAt",
] as const;

const moreCount = computed(() => moreKeys.filter(isSet).length);
const activeCount = computed(
  () =>
    moreCount.value + (["labels", "jobTypeIds", "executionStatuses"] as const).filter(isSet).length,
);

// Filters from a shared link should be visible.
const showMore = ref(moreCount.value > 0);

/** Selected job types that are not known (yet) are listed as well, so that they can be displayed. */
const jobTypeOptions = computed(() => [
  ...jobTypes.value,
  ...model.value.jobTypeIds.filter(id => !byId.value.has(id)).map(id => ({ id, description: "" })),
]);

function clear() {
  const cleared = create(JobFiltersSchema);
  // Keep the hidden (fixed) filters.
  for (const key of props.hidden ?? []) {
    (cleared as any)[key] = model.value[key];
  }
  model.value = cleared;
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <div class="flex flex-wrap items-center gap-2">
      <LabelFilterInput
        v-if="visible('labels')"
        :model-value="model.labels"
        class="min-w-72 flex-[1_1_24rem]"
        @update:model-value="set('labels', $event)"
      />
      <MultiSelect
        v-if="visible('jobTypeIds')"
        :model-value="model.jobTypeIds"
        :options="jobTypeOptions"
        option-label="id"
        option-value="id"
        placeholder="Job types"
        filter
        :loading="!loaded"
        :max-selected-labels="2"
        :virtual-scroller-options="jobTypeOptions.length > 50 ? { itemSize: 40 } : undefined"
        show-clear
        class="w-64"
        @update:model-value="set('jobTypeIds', $event)"
      />
      <MultiSelect
        v-if="visible('executionStatuses')"
        :model-value="model.executionStatuses"
        :options="executionStatusOptions"
        option-label="label"
        option-value="value"
        placeholder="Status"
        :max-selected-labels="2"
        show-clear
        class="w-56"
        @update:model-value="set('executionStatuses', $event)"
      >
        <template #option="{ option }">
          <Tag :value="option.label" :severity="option.severity" :icon="option.icon" />
        </template>
      </MultiSelect>
      <Button
        label="More filters"
        :icon="showMore ? 'pi pi-chevron-up' : 'pi pi-filter'"
        severity="secondary"
        text
        :badge="moreCount > 0 ? String(moreCount) : undefined"
        @click="showMore = !showMore"
      />
      <Button
        v-if="activeCount > 0"
        label="Clear"
        icon="pi pi-filter-slash"
        severity="secondary"
        text
        @click="clear"
      />
    </div>

    <div v-if="showMore" class="grid grid-cols-1 gap-3 md:grid-cols-2">
      <template v-for="idFilter in idFilters" :key="idFilter.key">
        <div v-if="visible(idFilter.key)" class="flex flex-col gap-1">
          <label class="text-sm text-muted-color">{{ idFilter.label }}</label>
          <AutoComplete
            :model-value="model[idFilter.key]"
            multiple
            :typeahead="false"
            placeholder="Type an ID and press enter"
            fluid
            @update:model-value="set(idFilter.key, $event ?? [])"
          />
        </div>
      </template>
      <div v-if="visible('targetExecutionTime')" class="flex flex-col gap-1">
        <label class="text-sm text-muted-color">Target execution time</label>
        <TimeRangeInput
          :model-value="model.targetExecutionTime"
          @update:model-value="set('targetExecutionTime', $event)"
        />
      </div>
      <div v-if="visible('createdAt')" class="flex flex-col gap-1">
        <label class="text-sm text-muted-color">Created at</label>
        <TimeRangeInput
          :model-value="model.createdAt"
          @update:model-value="set('createdAt', $event)"
        />
      </div>
    </div>
  </div>
</template>
