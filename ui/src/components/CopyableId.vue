<script setup lang="ts">
import { computed, ref } from "vue";
import type { RouteLocationRaw } from "vue-router";
import { useToast } from "primevue/usetoast";
import { copyToClipboard } from "../util/clipboard";
import { useErrorToast } from "../util/errors";
import { shortId } from "../util/format";

const props = defineProps<{
  id: string;
  /** Only display the distinguishing part of the ID. */
  short?: boolean;
  /** Makes the ID a link, a separate button is shown for copying. */
  to?: RouteLocationRaw;
}>();

const toast = useToast();
const reportError = useErrorToast();

const copied = ref(false);
const label = computed(() => (props.short ? shortId(props.id) : props.id));

async function copy() {
  try {
    await copyToClipboard(props.id);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
    toast.add({
      severity: "secondary",
      summary: "Copied to clipboard",
      detail: props.id,
      life: 1500,
    });
  } catch (error) {
    reportError(error, "Failed to copy");
  }
}
</script>

<template>
  <span class="inline-flex min-w-0 items-center gap-0.5 whitespace-nowrap">
    <template v-if="props.to">
      <RouterLink
        v-tooltip.top="props.id"
        :to="props.to"
        class="truncate font-mono text-sm text-primary hover:underline"
      >
        {{ label }}
      </RouterLink>
      <Button
        v-tooltip.top="'Copy ID'"
        :icon="copied ? 'pi pi-check' : 'pi pi-copy'"
        severity="secondary"
        text
        rounded
        size="small"
        class="h-6! w-6! shrink-0"
        aria-label="Copy ID"
        @click="copy"
      />
    </template>
    <button
      v-else
      v-tooltip.top="`${props.id}\nClick to copy`"
      type="button"
      class="inline-flex min-w-0 cursor-copy items-center gap-1 rounded-border font-mono text-sm hover:text-primary"
      @click="copy"
    >
      <span class="truncate">{{ label }}</span>
      <i :class="copied ? 'pi pi-check' : 'pi pi-copy'" class="shrink-0 text-xs text-muted-color" />
    </button>
  </span>
</template>
