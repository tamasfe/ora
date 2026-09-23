<script setup lang="ts">
import { useRoute } from "vue-router";
import { refreshIntervalOptions, usePolling } from "../util/polling";
import { useLocalStorageRef } from "../util/storage";

const props = defineProps<{
  loading?: boolean;
  /**
   * Distinguishes multiple controls on the same page,
   * the setting is remembered per page type and ID.
   */
  id?: string;
}>();

const emit = defineEmits<{
  refresh: [];
}>();

const route = useRoute();

const interval = useLocalStorageRef(
  `refresh:${String(route.name)}${props.id ? `:${props.id}` : ""}`,
  0,
  (value): value is number => refreshIntervalOptions.some(option => option.value === value),
);

usePolling(() => emit("refresh"), interval);
</script>

<template>
  <div class="flex items-center gap-1">
    <Button
      v-tooltip.bottom="'Refresh'"
      icon="pi pi-refresh"
      :loading="props.loading && interval === 0"
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
