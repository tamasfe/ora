<script setup lang="ts">
import { computed } from "vue";
import type { LoadState } from "../util";
import { errorMessage } from "../util/errors";
import { formatClock, formatLatency, formatSeconds } from "../util/format";
import { useNow } from "../util/time";

const props = defineProps<{
  state: LoadState;
  verb: "list" | "count" | "load";
  /** Also show when the data was last updated. */
  updated?: boolean;
}>();

const verbs = {
  list: ["Listing", "Listed"],
  count: ["Counting", "Counted"],
  load: ["Loading", "Loaded"],
} as const;

/** The elapsed time is only shown for requests that take a while. */
const showElapsedAfter = 1000;

const now = useNow(1000);

const elapsed = computed(() => {
  const startedAt = props.state.startedAt.value;

  if (!props.state.loading.value || startedAt === undefined) {
    return undefined;
  }

  const ms = now.value - startedAt;
  return ms >= showElapsedAfter ? ms : undefined;
});

const error = computed(() => props.state.error.value);
const duration = computed(() => props.state.duration.value);
const loadedAt = computed(() => props.state.loadedAt.value);
</script>

<template>
  <span
    v-if="props.state.enabled.value"
    class="inline-block h-4 text-xs leading-4 whitespace-nowrap text-muted-color tabular-nums"
  >
    <template v-if="elapsed !== undefined">
      <i class="pi pi-spin pi-spinner mr-1 text-[0.625rem]!" />{{ verbs[props.verb][0] }}…
      {{ formatSeconds(elapsed) }}
    </template>
    <span v-else-if="error" v-tooltip.top="errorMessage(error)" class="text-red-500">
      <i class="pi pi-exclamation-triangle mr-1 text-[0.625rem]!" />{{ verbs[props.verb][0] }}
      failed
    </span>
    <template v-else-if="duration !== undefined">
      {{ verbs[props.verb][1] }} in {{ formatLatency(duration)
      }}<template v-if="props.updated && loadedAt !== undefined">
        · updated {{ formatClock(loadedAt) }}</template
      >
    </template>
  </span>
</template>
