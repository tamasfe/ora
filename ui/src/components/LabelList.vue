<script setup lang="ts" generic="L extends { key: string; value?: string }">
import { computed, ref } from "vue";
import type { RouteLocationRaw } from "vue-router";

const props = withDefaults(
  defineProps<{
    labels: L[];
    /** The number of labels shown before the rest is collapsed. */
    max?: number;
    /** Makes each label a link, e.g. to a filtered list. */
    link?: (label: L) => RouteLocationRaw;
    /** Labels emit `select` when clicked. */
    selectable?: boolean;
    /** Explains what clicking a label does. */
    hint?: string;
    size?: "small" | "normal";
  }>(),
  { max: Infinity, size: "small" },
);

const emit = defineEmits<{
  select: [label: L];
}>();

const expanded = ref(false);

const visible = computed(() =>
  expanded.value ? props.labels : props.labels.slice(0, Math.max(props.max, 0)),
);
const hidden = computed(() => props.labels.length - visible.value.length);
</script>

<template>
  <div class="flex min-w-0 flex-wrap items-center gap-1">
    <LabelChip
      v-for="label in visible"
      :key="label.key"
      :label="label"
      :to="props.link?.(label)"
      :clickable="props.selectable"
      :hint="props.hint"
      :size="props.size"
      @click="emit('select', label)"
    />
    <button
      v-if="hidden > 0"
      v-tooltip.top="`Show ${hidden} more label${hidden === 1 ? '' : 's'}`"
      type="button"
      class="cursor-pointer rounded-border px-1 text-xs text-primary hover:underline"
      @click="expanded = true"
    >
      +{{ hidden }}
    </button>
    <button
      v-else-if="expanded && props.labels.length > props.max"
      type="button"
      class="cursor-pointer rounded-border px-1 text-xs text-primary hover:underline"
      @click="expanded = false"
    >
      Less
    </button>
  </div>
</template>
