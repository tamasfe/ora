<script setup lang="ts">
import { formatCount } from "../util/format";

const props = defineProps<{
  /** The value, a placeholder of the same size is shown while it is unknown. */
  value?: number;
  /** Loading failed, shown instead of the placeholder. */
  failed?: boolean;
}>();
</script>

<template>
  <span class="block h-9 text-3xl leading-9 font-semibold tabular-nums">
    <template v-if="props.value !== undefined">{{ formatCount(props.value) }}</template>
    <span v-else-if="props.failed" class="text-muted-color">–</span>
    <Skeleton v-else width="4rem" height="2.25rem" />
  </span>
</template>
