<script setup lang="ts">
import { computed, ref } from "vue";
import { create } from "@bufbuild/protobuf";
import { ScheduleFiltersSchema, type ScheduleFilters } from "../api/ora/admin/v1/schedules_pb";
import { useJobTypes } from "../util/data";
import { scheduleStatusOptions } from "../util/status";

const props = defineProps<{
  /** Filters that are not editable (e.g. fixed by the parent view). */
  hidden?: (keyof ScheduleFilters)[];
}>();

const model = defineModel<ScheduleFilters>({ required: true });

const { jobTypes, byId, loaded } = useJobTypes();

function visible(key: keyof ScheduleFilters) {
  return !props.hidden?.includes(key);
}

function set<K extends keyof ScheduleFilters>(key: K, value: ScheduleFilters[K]) {
  model.value = { ...model.value, [key]: value };
}

function isSet(key: keyof ScheduleFilters): boolean {
  const value = model.value[key];
  return visible(key) && (Array.isArray(value) ? value.length > 0 : value !== undefined);
}

/** Filters in the collapsible section. */
const moreKeys = ["scheduleIds", "createdAt"] as const;

const moreCount = computed(() => moreKeys.filter(isSet).length);
const activeCount = computed(
  () => moreCount.value + (["labels", "jobTypeIds", "statuses"] as const).filter(isSet).length,
);

// Filters from a shared link should be visible.
const showMore = ref(moreCount.value > 0);

/** Selected job types that are not known (yet) are listed as well, so that they can be displayed. */
const jobTypeOptions = computed(() => [
  ...jobTypes.value,
  ...model.value.jobTypeIds.filter(id => !byId.value.has(id)).map(id => ({ id, description: "" })),
]);

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
        v-if="visible('statuses')"
        :model-value="model.statuses"
        :options="scheduleStatusOptions"
        option-label="label"
        option-value="value"
        placeholder="Status"
        show-clear
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
      <div v-if="visible('scheduleIds')" class="flex flex-col gap-1">
        <label class="text-sm text-muted-color">Schedule IDs</label>
        <AutoComplete
          :model-value="model.scheduleIds"
          multiple
          :typeahead="false"
          placeholder="Type an ID and press enter"
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
    </div>
  </div>
</template>
