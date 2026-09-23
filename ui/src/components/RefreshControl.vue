<script setup lang="ts">
import { refreshIntervalOptions } from "../util/polling";

const props = defineProps<{
  /** A refresh is in progress, refreshing again is not possible until it finishes. */
  loading?: boolean;
}>();

/** The auto refresh interval in milliseconds, zero is off. */
const interval = defineModel<number>({ required: true });

const emit = defineEmits<{
  refresh: [];
}>();
</script>

<template>
  <div class="flex items-center gap-1">
    <Button
      v-tooltip.bottom="'Refresh'"
      icon="pi pi-refresh"
      :loading="props.loading"
      severity="secondary"
      text
      rounded
      size="small"
      aria-label="Refresh"
      @click="emit('refresh')"
    />
    <SelectButton
      v-model="interval"
      v-tooltip.bottom="'Auto refresh'"
      :options="refreshIntervalOptions"
      option-label="label"
      option-value="value"
      :allow-empty="false"
      size="small"
    />
  </div>
</template>
