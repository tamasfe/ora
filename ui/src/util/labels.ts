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
  return rows.map(row =>
    create(LabelFilterSchema, { key: row.key, value: row.value === "" ? undefined : row.value }),
  );
}
