<script setup lang="ts">
import { nextTick, ref } from "vue";
import type { LabelFilter } from "../api/ora/common/v1/label_pb";
import {
  formatLabel,
  parseLabel,
  parseLabelLines,
  rowToLabelFilter,
  withLabelFilter,
  type LabelRow,
} from "../util/labels";

const props = withDefaults(
  defineProps<{
    placeholder?: string;
    inputId?: string;
  }>(),
  { placeholder: "Filter by labels: key=value, or key for any value" },
);

const model = defineModel<LabelFilter[]>({ required: true });

const input = ref<HTMLInputElement>();
const text = ref("");
/** The index of the filter being edited in the input, it is hidden until committed. */
const editing = ref<number>();

function focus() {
  nextTick(() => input.value?.focus());
}

/** Adds filters, replacing existing ones with the same keys. */
function add(rows: LabelRow[]) {
  let filters = model.value;
  for (const row of rows) {
    filters = withLabelFilter(filters, rowToLabelFilter(row));
  }
  model.value = filters;
}

/** Applies the input, filters only change on commit to avoid reloading on every keystroke. */
function commit() {
  const row = parseLabel(text.value);
  const index = editing.value;
  text.value = "";
  editing.value = undefined;

  if (index === undefined) {
    if (row) {
      add([row]);
    }
    return;
  }

  const rest = model.value.filter((_, i) => i !== index);
  if (!row) {
    model.value = rest;
    return;
  }

  // Keep the position of the edited filter, other filters with the same key are replaced.
  const filter = rowToLabelFilter(row);
  const others = rest.filter(f => f.key !== filter.key);
  const position = Math.min(index, others.length);
  model.value = [...others.slice(0, position), filter, ...others.slice(position)];
}

function cancel() {
  text.value = "";
  editing.value = undefined;
}

function edit(index: number) {
  if (text.value.trim() !== "" || editing.value !== undefined) {
    commit();
  }

  const filter = model.value[index];
  if (filter) {
    editing.value = index;
    text.value = formatLabel(filter);
    focus();
  }
}

function remove(index: number) {
  model.value = model.value.filter((_, i) => i !== index);
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Enter") {
    event.preventDefault();
    commit();
  } else if (event.key === "Escape" && editing.value !== undefined) {
    event.preventDefault();
    cancel();
  } else if (
    event.key === "Backspace" &&
    text.value === "" &&
    editing.value === undefined &&
    model.value.length > 0
  ) {
    // Edit the last filter instead of removing it right away.
    event.preventDefault();
    edit(model.value.length - 1);
  }
}

function onPaste(event: ClipboardEvent) {
  const pasted = event.clipboardData?.getData("text") ?? "";

  // Single labels are pasted into the input as usual.
  if (!/\r?\n/.test(pasted.trim())) {
    return;
  }

  event.preventDefault();
  if (text.value.trim() !== "" || editing.value !== undefined) {
    commit();
  }
  add(parseLabelLines(pasted));
}
</script>

<template>
  <div
    class="label-filter-input flex min-h-10 min-w-0 flex-wrap items-center gap-1 px-2 py-1"
    @click="input?.focus()"
  >
    <i class="pi pi-tags mx-1 text-muted-color" />
    <template v-for="(filter, index) in model" :key="filter.key">
      <LabelChip
        v-if="index !== editing"
        :label="filter"
        size="normal"
        clickable
        removable
        hint="Click to edit"
        @click="edit(index)"
        @remove="remove(index)"
      />
    </template>
    <input
      :id="props.inputId"
      ref="input"
      v-model="text"
      type="text"
      autocomplete="off"
      spellcheck="false"
      class="min-w-48 flex-1 bg-transparent py-1 font-mono text-sm outline-none placeholder:font-sans placeholder:text-(--p-form-field-placeholder-color)"
      :placeholder="model.length === 0 ? props.placeholder : 'Add label filter'"
      aria-label="Label filters"
      @keydown="onKeydown"
      @paste="onPaste"
      @blur="commit"
    />
  </div>
</template>

<style scoped>
.label-filter-input {
  background: var(--p-form-field-background);
  color: var(--p-form-field-color);
  border: 1px solid var(--p-form-field-border-color);
  border-radius: var(--p-form-field-border-radius);
  box-shadow: var(--p-form-field-shadow);
  transition: border-color var(--p-form-field-transition-duration);
  cursor: text;
}

.label-filter-input:hover {
  border-color: var(--p-form-field-hover-border-color);
}

.label-filter-input:focus-within {
  border-color: var(--p-form-field-focus-border-color);
}
</style>
