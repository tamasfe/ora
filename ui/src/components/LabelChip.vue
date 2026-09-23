<script setup lang="ts">
import { computed } from "vue";
import type { RouteLocationRaw } from "vue-router";
import { formatLabel } from "../util/labels";

const props = withDefaults(
  defineProps<{
    /** A label, or a label filter without a value matching any value. */
    label: { key: string; value?: string };
    /** Makes the chip a link. */
    to?: RouteLocationRaw;
    /** Makes the chip a button that emits `click`. */
    clickable?: boolean;
    /** Shows a button that emits `remove`. */
    removable?: boolean;
    /** Explains what clicking the chip does, shown with the full label on hover. */
    hint?: string;
    size?: "small" | "normal";
  }>(),
  { size: "small" },
);

const emit = defineEmits<{
  click: [];
  remove: [];
}>();

const text = computed(() => formatLabel(props.label));
const tooltip = computed(() => ({
  value: props.hint ? `${text.value}\n${props.hint}` : text.value,
  showDelay: 400,
}));
</script>

<template>
  <span
    class="inline-flex max-w-full min-w-0 items-center rounded-border bg-surface-100 font-mono text-color dark:bg-surface-800"
    :class="props.size === 'small' ? 'text-xs leading-5' : 'text-sm leading-6'"
  >
    <RouterLink
      v-if="props.to"
      v-tooltip.top="tooltip"
      :to="props.to"
      class="min-w-0 truncate px-1.5 hover:underline"
    >
      <span class="text-muted-color">{{ props.label.key }}</span
      ><template v-if="props.label.value !== undefined"
        ><span class="text-muted-color">=</span>{{ props.label.value }}</template
      >
    </RouterLink>
    <button
      v-else-if="props.clickable"
      v-tooltip.top="tooltip"
      type="button"
      class="min-w-0 cursor-pointer truncate px-1.5 text-left hover:underline"
      @click="emit('click')"
    >
      <span class="text-muted-color">{{ props.label.key }}</span
      ><template v-if="props.label.value !== undefined"
        ><span class="text-muted-color">=</span>{{ props.label.value }}</template
      >
    </button>
    <span v-else v-tooltip.top="tooltip" class="min-w-0 truncate px-1.5">
      <span class="text-muted-color">{{ props.label.key }}</span
      ><template v-if="props.label.value !== undefined"
        ><span class="text-muted-color">=</span>{{ props.label.value }}</template
      >
    </span>
    <button
      v-if="props.removable"
      type="button"
      class="flex shrink-0 cursor-pointer items-center self-stretch pr-1.5 pl-0.5 text-muted-color hover:text-color"
      :aria-label="`Remove ${text}`"
      @click.stop="emit('remove')"
    >
      <i class="pi pi-times text-[0.625rem]!" />
    </button>
  </span>
</template>
