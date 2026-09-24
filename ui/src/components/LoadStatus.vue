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
  /** Prefix the duration with an icon of the verb, to tell apart statuses shown together. */
  icon?: boolean;
}>();

const verbs = {
  list: { active: "Listing", done: "Listed", icon: "pi-list" },
  count: { active: "Counting", done: "Counted", icon: "pi-hashtag" },
  load: { active: "Loading", done: "Loaded", icon: "pi-download" },
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

/** Only the durations are shown, the full description is in the tooltip. */
const description = computed(() => {
  const verb = verbs[props.verb];

  if (elapsed.value !== undefined) {
    return `${verb.active}… ${formatSeconds(elapsed.value)}`;
  }

  if (error.value) {
    return `${verb.active} failed: ${errorMessage(error.value)}`;
  }

  if (duration.value !== undefined && loadedAt.value !== undefined) {
    return `${verb.done} in ${formatLatency(duration.value)} at ${formatClock(loadedAt.value)}`;
  }

  return undefined;
});
</script>

<template>
  <span
    v-if="props.state.enabled.value"
    v-tooltip.top="description"
    :aria-label="description"
    class="inline-flex h-4 items-center gap-1 text-xs leading-4 whitespace-nowrap text-muted-color tabular-nums"
  >
    <template v-if="elapsed !== undefined">
      <i class="pi pi-spin pi-spinner text-[0.625rem]!" />{{ formatSeconds(elapsed) }}
    </template>
    <i v-else-if="error" class="pi pi-exclamation-triangle text-[0.625rem]! text-red-500" />
    <template v-else-if="duration !== undefined">
      <i v-if="props.icon" class="pi text-[0.625rem]!" :class="verbs[props.verb].icon" />
      {{ formatLatency(duration) }}
      <template v-if="props.updated && loadedAt !== undefined">
        · updated {{ formatClock(loadedAt) }}
      </template>
    </template>
  </span>
</template>
