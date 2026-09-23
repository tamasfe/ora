<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { basicSetup } from "codemirror";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { json } from "@codemirror/lang-json";
import { tags } from "@lezer/highlight";
import { jsonSchema, updateSchema } from "codemirror-json-schema";
import { disableErrorLogging } from "best-effort-json-parser";
import type { JsonSchema } from "../util/schema";

// Used by codemirror-json-schema, it logs every partial document to the console otherwise.
disableErrorLogging();

const props = withDefaults(
  defineProps<{
    /** JSON schema used for autocompletion, hover information and validation. */
    schema?: JsonSchema;
    readonly?: boolean;
    minHeight?: string;
    maxHeight?: string;
  }>(),
  { minHeight: "4rem", maxHeight: "32rem" },
);

const model = defineModel<string>({ default: "" });

const container = ref<HTMLElement>();
let view: EditorView | undefined;

const readOnlyCompartment = new Compartment();

const theme = EditorView.theme({
  "&": {
    backgroundColor: "var(--p-form-field-background)",
    color: "var(--p-form-field-color)",
    border: "1px solid var(--p-form-field-border-color)",
    borderRadius: "var(--p-form-field-border-radius)",
    fontSize: "0.875rem",
    overflow: "hidden",
  },
  "&.cm-focused": {
    outline: "none",
    borderColor: "var(--p-form-field-focus-border-color)",
  },
  ".cm-scroller": {
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
  },
  ".cm-content": { caretColor: "var(--p-text-color)" },
  ".cm-cursor": { borderLeftColor: "var(--p-text-color)" },
  ".cm-gutters": {
    backgroundColor: "var(--p-content-hover-background)",
    color: "var(--p-text-muted-color)",
    borderRight: "1px solid var(--p-content-border-color)",
  },
  ".cm-activeLine, .cm-activeLineGutter": {
    backgroundColor: "color-mix(in srgb, var(--p-primary-color) 6%, transparent)",
  },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "color-mix(in srgb, var(--p-primary-color) 25%, transparent) !important",
  },
  ".cm-tooltip": {
    backgroundColor: "var(--p-overlay-popover-background)",
    color: "var(--p-overlay-popover-color)",
    border: "1px solid var(--p-overlay-popover-border-color)",
    borderRadius: "var(--p-overlay-popover-border-radius)",
    padding: "0.25rem",
    maxWidth: "32rem",
  },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    background: "var(--p-highlight-background)",
    color: "var(--p-highlight-color)",
  },
  ".cm-diagnostic": { padding: "0.25rem 0.5rem" },
});

// Mid-range colors are readable in both light and dark mode.
const highlight = HighlightStyle.define([
  { tag: tags.propertyName, color: "var(--p-blue-500)" },
  { tag: tags.string, color: "var(--p-green-600)" },
  { tag: tags.number, color: "var(--p-orange-500)" },
  { tag: [tags.bool, tags.null], color: "var(--p-purple-500)" },
  { tag: tags.invalid, color: "var(--p-red-500)" },
]);

function readOnlyExtensions(readonly: boolean) {
  return [EditorState.readOnly.of(readonly), EditorView.editable.of(!readonly)];
}

onMounted(() => {
  view = new EditorView({
    parent: container.value!,
    state: EditorState.create({
      doc: model.value,
      extensions: [
        basicSetup,
        props.readonly ? json() : jsonSchema(props.schema as any),
        theme,
        syntaxHighlighting(highlight),
        EditorView.lineWrapping,
        readOnlyCompartment.of(readOnlyExtensions(props.readonly)),
        EditorView.theme({
          ".cm-content, .cm-gutter": { minHeight: props.minHeight },
          ".cm-scroller": { maxHeight: props.maxHeight, overflow: "auto" },
        }),
        EditorView.updateListener.of(update => {
          if (update.docChanged) {
            model.value = update.state.doc.toString();
          }
        }),
      ],
    }),
  });
});

onBeforeUnmount(() => {
  view?.destroy();
});

watch(model, value => {
  if (view && value !== view.state.doc.toString()) {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: value },
    });
  }
});

watch(
  () => props.schema,
  schema => {
    if (view && !props.readonly) {
      updateSchema(view, schema as any);
    }
  },
);

watch(
  () => props.readonly,
  readonly => {
    view?.dispatch({
      effects: readOnlyCompartment.reconfigure(readOnlyExtensions(readonly)),
    });
  },
);
</script>

<template>
  <div ref="container" />
</template>
