<script setup lang="ts">
defineProps<{
  active?: boolean;
}>();
</script>

<template>
  <!-- The space is always reserved, so that showing the bar doesn't move the content. -->
  <div class="pointer-events-none h-0.5 overflow-hidden" :aria-busy="active">
    <ProgressBar v-if="active" mode="indeterminate" class="h-0.5! rounded-none!" />
  </div>
</template>
