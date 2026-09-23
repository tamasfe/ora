<script setup lang="ts">
import { computed } from "vue";
import { create } from "@bufbuild/protobuf";
import { timestampDate, timestampFromDate } from "@bufbuild/protobuf/wkt";
import { TimeRangeSchema, type TimeRange } from "../api/ora/common/v1/time_range_pb";

const props = withDefaults(
  defineProps<{
    startPlaceholder?: string;
    endPlaceholder?: string;
  }>(),
  { startPlaceholder: "From", endPlaceholder: "Until" },
);

const model = defineModel<TimeRange | undefined>();

function update(key: "start" | "end", date: Date | null | undefined) {
  const next = {
    start: model.value?.start,
    end: model.value?.end,
    [key]: date instanceof Date ? timestampFromDate(date) : undefined,
  };

  model.value = next.start || next.end ? create(TimeRangeSchema, next) : undefined;
}

const start = computed({
  get: () => (model.value?.start ? timestampDate(model.value.start) : null),
  set: (date: Date | null) => update("start", date),
});

const end = computed({
  get: () => (model.value?.end ? timestampDate(model.value.end) : null),
  set: (date: Date | null) => update("end", date),
});
</script>

<template>
  <div class="flex flex-wrap items-center gap-2">
    <DatePicker
      v-model="start"
      show-time
      hour-format="24"
      show-button-bar
      :placeholder="props.startPlaceholder"
      size="small"
      class="min-w-48 flex-1"
    />
    <span class="text-muted-color">–</span>
    <DatePicker
      v-model="end"
      show-time
      hour-format="24"
      show-button-bar
      :placeholder="props.endPlaceholder"
      size="small"
      class="min-w-48 flex-1"
    />
  </div>
</template>
