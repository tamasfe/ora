<script setup lang="ts">
import { computed, ref } from "vue";
import { create } from "@bufbuild/protobuf";
import { ScheduleFiltersSchema, type ScheduleFilters } from "../api/ora/admin/v1/schedules_pb";
import { useJobTypes } from "../util/data";
import { labelsToRows, rowsToLabelFilters } from "../util/labels";
import { scheduleStatusOptions } from "../util/status";

const props = defineProps<{
  /** Filters that are not editable (e.g. fixed by the parent view). */
  hidden?: (keyof ScheduleFilters)[];
}>();

const model = defineModel<ScheduleFilters>({ required: true });

const { jobTypes } = useJobTypes();

const showMore = ref(false);

function visible(key: keyof ScheduleFilters) {
  return !props.hidden?.includes(key);
}

function set<K extends keyof ScheduleFilters>(key: K, value: ScheduleFilters[K]) {
  model.value = { ...model.value, [key]: value };
}

const activeCount = computed(() => {
  const f = model.value;
  return [
    f.scheduleIds.length,
    f.jobTypeIds.length,
    f.statuses.length,
    f.labels.length,
    f.createdAt ? 1 : 0,
  ].filter(n => n > 0).length;
});

const labelRows = computed({
  get: () => labelsToRows(model.value.labels),
  set: rows => set("labels", rowsToLabelFilters(rows)),
});

function clear() {
  const cleared = create(ScheduleFiltersSchema);
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
        v-if="visible('statuses')"
        :model-value="model.statuses"
        :options="scheduleStatusOptions"
        option-label="label"
        option-value="value"
        placeholder="Status"
        show-clear
        size="small"
        class="w-48"
        @update:model-value="set('statuses', $event)"
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
      <div v-if="visible('scheduleIds')" class="flex flex-col gap-1">
        <label class="text-sm text-muted-color">Schedule IDs</label>
        <AutoComplete
          :model-value="model.scheduleIds"
          multiple
          :typeahead="false"
          placeholder="Type an ID and press enter"
          size="small"
          fluid
          @update:model-value="set('scheduleIds', $event ?? [])"
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
