<script setup lang="ts">
import { computed } from "vue";

const props = withDefaults(
  defineProps<{
    text: string;
    /** Characters after which the text prefers to break. */
    separators?: string;
  }>(),
  { separators: "./" },
);

// Long identifiers (e.g. `a.b.SomeJobType` or `service/1.2.3`) break at separators first,
// anywhere else only if a part still does not fit.
const parts = computed(() => {
  const parts: string[] = [];
  let start = 0;

  for (let i = 0; i < props.text.length; i++) {
    if (props.separators.includes(props.text[i])) {
      parts.push(props.text.slice(start, i + 1));
      start = i + 1;
    }
  }

  if (start < props.text.length) {
    parts.push(props.text.slice(start));
  }

  return parts;
});
</script>

<template>
  <span class="[overflow-wrap:anywhere]">
    <template v-for="(part, index) in parts" :key="index">{{ part }}<wbr /></template>
  </span>
</template>
