<script setup lang="ts">
import { computed } from "vue";
import { timestampMs, type Timestamp } from "@bufbuild/protobuf/wkt";
import { formatMs, formatTimestamp } from "../util/format";
import { useNow } from "../util/time";

const props = defineProps<{
  start?: Timestamp;
  end?: Timestamp;
  /** Counts up from the start while there is no end, e.g. for executions in progress. */
  live?: boolean;
}>();

const now = useNow(1000);

const running = computed(() => !!props.start && !props.end && props.live);

const ms = computed(() => {
  if (!props.start) {
    return undefined;
  }

  // The clock is only read while running, so finished durations don't re-render every second.
  const end = props.end ? timestampMs(props.end) : props.live ? now.value : undefined;
  if (end === undefined) {
    return undefined;
  }

  return Math.max(0, end - timestampMs(props.start));
});

const label = computed(() => {
  if (ms.value === undefined) {
    return "-";
  }

  // Whole seconds while running, so that the value doesn't jitter.
  return formatMs(running.value ? Math.floor(ms.value / 1000) * 1000 : ms.value);
});

const tooltip = computed(() => {
  if (!props.start) {
    return undefined;
  }

  return props.end
    ? `${formatTimestamp(props.start)} – ${formatTimestamp(props.end)}`
    : `Started ${formatTimestamp(props.start)}`;
});
</script>

<template>
  <span
    v-tooltip.top="tooltip ? { value: tooltip, showDelay: 400 } : undefined"
    class="whitespace-nowrap tabular-nums"
    :class="{ 'text-muted-color': running }"
  >
    <i v-if="running" class="pi pi-stopwatch mr-1 text-xs" />{{ label }}
  </span>
</template>
