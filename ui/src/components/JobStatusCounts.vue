<script setup lang="ts">
import { computed } from "vue";
import type { RouteLocationRaw } from "vue-router";
import type { ExecutionStatus } from "../api/ora/admin/v1/executions_pb";
import type { StatusCounts } from "../util/counts";
import { formatCompactCount, formatCount } from "../util/format";
import { executionStatusOptions } from "../util/status";

const props = defineProps<{
  /** The number of jobs by status, a placeholder is shown while they are unknown. */
  counts?: StatusCounts;
  /** Loading failed, shown instead of the placeholder. */
  failed?: boolean;
  /** The link of each status, e.g. to the jobs with the status. */
  link: (status: ExecutionStatus) => RouteLocationRaw;
  /** Keep the statuses on one line, e.g. in tables. */
  nowrap?: boolean;
}>();

/** Only statuses with jobs are shown. */
const statuses = computed(() =>
  executionStatusOptions
    .map(option => ({
      ...option,
      // Many spinning icons are distracting, this is not about progress anyway.
      icon: option.icon.replace("pi-spin ", ""),
      count: props.counts?.get(option.value) ?? 0,
    }))
    .filter(option => option.count > 0),
);
</script>

<template>
  <div
    class="flex min-h-6 items-center gap-1"
    :class="props.nowrap ? 'flex-nowrap whitespace-nowrap' : 'flex-wrap'"
  >
    <template v-if="props.counts === undefined">
      <span v-if="props.failed" class="text-muted-color">–</span>
      <Skeleton v-else width="8rem" height="1.5rem" />
    </template>
    <span v-else-if="statuses.length === 0" class="text-sm text-muted-color">No jobs</span>
    <RouterLink
      v-for="status in statuses"
      :key="status.value"
      v-tooltip.top="`${status.label}: ${formatCount(status.count)}`"
      :to="props.link(status.value)"
      :aria-label="`${status.label}: ${formatCount(status.count)}`"
    >
      <Tag
        :value="formatCompactCount(status.count)"
        :severity="status.severity"
        :icon="status.icon"
        class="font-normal! tabular-nums"
      />
    </RouterLink>
  </div>
</template>
