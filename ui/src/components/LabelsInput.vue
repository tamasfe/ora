<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from "vue";
import { parseLabelLines, type LabelRow } from "../util/labels";

const model = defineModel<LabelRow[]>({ default: () => [] });

const id = useId();

/** The rows being edited, there is always an empty row at the end for adding a label. */
const rows = ref<LabelRow[]>([]);

watch(
  model,
  value => {
    rows.value = [...value.map(({ key, value }) => ({ key, value })), { key: "", value: "" }];
  },
  { immediate: true },
);

const duplicateKeys = computed(() => {
  const seen = new Set<string>();
  const duplicates = new Set<string>();
  for (const row of rows.value) {
    const key = row.key.trim();
    if (key !== "") {
      (seen.has(key) ? duplicates : seen).add(key);
    }
  }
  return duplicates;
});

function inputId(index: number, field: "key" | "value") {
  return `${id}-${field}-${index}`;
}

function focus(index: number, field: "key" | "value") {
  nextTick(() => document.getElementById(inputId(index, field))?.focus());
}

/** Keeps an empty row at the end, called while typing. */
function ensureEmptyRow() {
  const last = rows.value[rows.value.length - 1];
  if (!last || last.key !== "" || last.value !== "") {
    rows.value.push({ key: "", value: "" });
  }
}

/**
 * Emits complete rows only, called on input changes (blur or enter).
 *
 * Nothing is emitted without changes, so that incomplete rows (e.g. a value without a key yet)
 * are not reset.
 */
function commit() {
  const labels = rows.value
    .filter(row => row.key.trim() !== "")
    .map(row => ({ key: row.key.trim(), value: row.value }));

  const unchanged =
    labels.length === model.value.length &&
    labels.every(
      (label, i) => label.key === model.value[i].key && label.value === model.value[i].value,
    );

  if (!unchanged) {
    model.value = labels;
  }
}

function remove(index: number) {
  rows.value.splice(index, 1);
  commit();
}

function onKeyEnter(index: number, field: "key" | "value") {
  commit();
  if (field === "key") {
    focus(index, "value");
  } else {
    focus(Math.min(index + 1, rows.value.length - 1), "key");
  }
}

/** Pasting `key=value` lines into a key input fills in rows. */
function onPaste(index: number, event: ClipboardEvent) {
  const text = event.clipboardData?.getData("text") ?? "";

  if (!text.includes("=") && !/\r?\n/.test(text.trim())) {
    return;
  }

  const pasted = parseLabelLines(text);
  if (pasted.length === 0) {
    return;
  }

  event.preventDefault();
  const current = rows.value[index];
  const replace = current.key.trim() === "" && current.value === "" ? 1 : 0;
  rows.value.splice(index + 1 - replace, replace, ...pasted);
  commit();
  focus(Math.min(index + pasted.length, rows.value.length - 1), "key");
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <div v-for="(row, index) in rows" :key="index" class="flex items-center gap-2">
      <InputText
        :id="inputId(index, 'key')"
        v-model="row.key"
        :placeholder="index === rows.length - 1 ? 'Add a label key' : 'Key'"
        :invalid="duplicateKeys.has(row.key.trim())"
        aria-label="Label key"
        class="w-2/5 min-w-0 font-mono!"
        @input="ensureEmptyRow"
        @change="commit"
        @keydown.enter.prevent="onKeyEnter(index, 'key')"
        @paste="onPaste(index, $event)"
      />
      <span class="text-muted-color">=</span>
      <InputText
        :id="inputId(index, 'value')"
        v-model="row.value"
        placeholder="Value"
        aria-label="Label value"
        class="min-w-0 flex-1 font-mono!"
        @input="ensureEmptyRow"
        @change="commit"
        @keydown.enter.prevent="onKeyEnter(index, 'value')"
      />
      <Button
        icon="pi pi-times"
        severity="secondary"
        text
        rounded
        aria-label="Remove label"
        :class="{ invisible: index === rows.length - 1 }"
        @click="remove(index)"
      />
    </div>
    <small v-if="duplicateKeys.size > 0" class="text-red-500">
      Label keys must be unique: {{ [...duplicateKeys].join(", ") }}
    </small>
    <small v-else class="text-muted-color">
      Paste <span class="font-mono">key=value</span> lines to add several labels at once.
    </small>
  </div>
</template>
