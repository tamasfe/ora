import { create } from "@bufbuild/protobuf";
import {
  LabelFilterSchema,
  LabelSchema,
  type Label,
  type LabelFilter,
} from "../api/ora/common/v1/label_pb";

/** An editable label, the value might be empty for label filters. */
export interface LabelRow {
  key: string;
  value: string;
}

export function labelsToRows(labels: (Label | LabelFilter)[]): LabelRow[] {
  return labels.map(label => ({ key: label.key, value: label.value ?? "" }));
}

export function rowsToLabels(rows: LabelRow[]): Label[] {
  return rows.map(row => create(LabelSchema, row));
}

export function rowsToLabelFilters(rows: LabelRow[]): LabelFilter[] {
  return rows.map(rowToLabelFilter);
}

/** Filters matching the exact labels. */
export function labelsToFilters(labels: Label[]): LabelFilter[] {
  return labels.map(label => create(LabelFilterSchema, { key: label.key, value: label.value }));
}

export function rowToLabelFilter(row: LabelRow): LabelFilter {
  return create(LabelFilterSchema, {
    key: row.key,
    value: row.value === "" ? undefined : row.value,
  });
}

/**
 * Formats a label as `key=value`,
 * label filters without a value (matching any value) as just `key`.
 */
export function formatLabel(label: { key: string; value?: string }): string {
  return label.value === undefined ? label.key : `${label.key}=${label.value}`;
}

/**
 * Parses `key=value` or just `key`, splitting at the first `=` like the CLI does,
 * an empty value means any value for filters.
 *
 * Returns `undefined` if the key is empty.
 */
export function parseLabel(text: string): LabelRow | undefined {
  const trimmed = text.trim();
  const separator = trimmed.indexOf("=");
  const key = (separator < 0 ? trimmed : trimmed.slice(0, separator)).trim();

  if (key === "") {
    return undefined;
  }

  return { key, value: separator < 0 ? "" : trimmed.slice(separator + 1) };
}

/**
 * Parses pasted labels, one `key=value` per line.
 */
export function parseLabelLines(text: string): LabelRow[] {
  return text
    .split(/\r?\n/)
    .map(parseLabel)
    .filter(label => label !== undefined);
}

/**
 * Adds a label filter, replacing an existing filter with the same key
 * (filters with the same key could never match together).
 */
export function withLabelFilter(filters: LabelFilter[], filter: LabelFilter): LabelFilter[] {
  const index = filters.findIndex(f => f.key === filter.key);

  if (index < 0) {
    return [...filters, filter];
  }

  return filters.map((f, i) => (i === index ? filter : f));
}
