<script setup lang="ts">
import { ref, watch } from "vue";
import type { LabelRow } from "../util/labels";

const props = defineProps<{
  /**
   * In filter mode the value is optional,
   * an empty value matches any value of the key.
   */
  filter?: boolean;
}>();

const model = defineModel<LabelRow[]>({ default: () => [] });

const rows = ref<LabelRow[]>([]);

watch(
  model,
  value => {
    rows.value = value.map(({ key, value }) => ({ key, value }));
  },
  { immediate: true },
);

/** Emits complete rows only, called on input changes (blur or enter). */
function commit() {
  model.value = rows.value
    .filter(row => row.key.trim() !== "")
    .map(row => ({ key: row.key.trim(), value: row.value }));
}

function add() {
  rows.value.push({ key: "", value: "" });
}

function remove(index: number) {
  rows.value.splice(index, 1);
  commit();
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <div v-for="(row, index) in rows" :key="index" class="flex items-center gap-2">
      <InputText
        v-model="row.key"
        placeholder="Key"
        size="small"
        class="min-w-0 flex-1"
        @change="commit"
        @keydown.enter="commit"
      />
      <span class="text-muted-color">=</span>
      <InputText
        v-model="row.value"
        :placeholder="props.filter ? 'Any value' : 'Value'"
        size="small"
        class="min-w-0 flex-1"
        @change="commit"
        @keydown.enter="commit"
      />
      <Button
        icon="pi pi-times"
        severity="secondary"
        text
        rounded
        size="small"
        aria-label="Remove label"
        @click="remove(index)"
      />
    </div>
    <div>
      <Button
        :label="props.filter ? 'Add label filter' : 'Add label'"
        icon="pi pi-plus"
        severity="secondary"
        text
        size="small"
        @click="add"
      />
    </div>
  </div>
</template>
