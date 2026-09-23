<script setup lang="ts">
import { computed, ref } from "vue";
import { create } from "@bufbuild/protobuf";
import { JobFiltersSchema, type JobFilters } from "../api/ora/admin/v1/jobs_pb";
import { useJobTypes } from "../util/data";
import { labelsToRows, rowsToLabelFilters } from "../util/labels";
import { executionStatusOptions } from "../util/status";

const props = defineProps<{
  /** Filters that are not editable (e.g. fixed by the parent view). */
  hidden?: (keyof JobFilters)[];
}>();

const model = defineModel<JobFilters>({ required: true });

const { jobTypes } = useJobTypes();

const showMore = ref(false);

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

const activeCount = computed(() => {
  const f = model.value;
  return [
    f.jobIds.length,
    f.jobTypeIds.length,
    f.scheduleIds.length,
    f.executorIds.length,
    f.executionIds.length,
    f.executionStatuses.length,
    f.labels.length,
    f.targetExecutionTime ? 1 : 0,
    f.createdAt ? 1 : 0,
  ].filter(n => n > 0).length;
});

const labelRows = computed({
  get: () => labelsToRows(model.value.labels),
  set: rows => set("labels", rowsToLabelFilters(rows)),
});

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
      <MultiSelect
        v-if="visible('jobTypeIds')"
        :model-value="model.jobTypeIds"
        :options="jobTypes"
        option-label="id"
        option-value="id"
        placeholder="Job types"
        filter
        :max-selected-labels="2"
        show-clear
        size="small"
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
        size="small"
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
        size="small"
        :badge="activeCount > 0 ? String(activeCount) : undefined"
        @click="showMore = !showMore"
      />
      <Button
        v-if="activeCount > 0"
        label="Clear"
        icon="pi pi-filter-slash"
        severity="secondary"
        text
        size="small"
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
            size="small"
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
      <div v-if="visible('labels')" class="flex flex-col gap-1">
        <label class="text-sm text-muted-color">Labels</label>
        <LabelsInput v-model="labelRows" filter />
      </div>
    </div>
  </div>
</template>
