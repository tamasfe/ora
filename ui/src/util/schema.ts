import { Validator, type OutputUnit } from "@cfworker/json-schema";

export type JsonSchema = Record<string, any>;

/**
 * Parses a JSON schema string, returns `undefined`
 * if the schema is missing or invalid.
 */
export function parseSchema(json?: string): JsonSchema | undefined {
  if (!json) {
    return undefined;
  }

  try {
    const schema = JSON.parse(json);
    return typeof schema === "object" && schema !== null ? schema : undefined;
  } catch {
    return undefined;
  }
}

function resolveRef(root: JsonSchema, ref: string): JsonSchema | undefined {
  if (!ref.startsWith("#")) {
    return undefined;
  }

  let current: any = root;

  for (const part of ref.slice(1).split("/").filter(Boolean)) {
    current = current?.[decodeURIComponent(part).replace(/~1/g, "/").replace(/~0/g, "~")];
  }

  return typeof current === "object" && current !== null ? current : undefined;
}

/**
 * Generates a minimal valid-looking value for the given schema.
 *
 * Only required object properties are included,
 * optional ones can be added with the help of autocompletion.
 */
export function schemaTemplate(
  schema: JsonSchema | boolean | undefined,
  root?: JsonSchema,
  depth = 0,
): unknown {
  if (typeof schema !== "object" || schema === null || depth > 16) {
    return null;
  }

  root ??= schema;

  if (schema.$ref) {
    const resolved = resolveRef(root, schema.$ref);
    const { $ref: _, ...rest } = schema;
    return schemaTemplate({ ...resolved, ...rest }, root, depth + 1);
  }

  if (schema.default !== undefined) {
    return schema.default;
  }

  if (schema.const !== undefined) {
    return schema.const;
  }

  if (Array.isArray(schema.enum) && schema.enum.length > 0) {
    return schema.enum[0];
  }

  if (Array.isArray(schema.allOf) && schema.allOf.length > 0) {
    const parts = schema.allOf.map((s: JsonSchema) => schemaTemplate(s, root, depth + 1));
    if (parts.every((p: unknown) => typeof p === "object" && p !== null && !Array.isArray(p))) {
      return Object.assign({}, ...parts);
    }
    return parts[0];
  }

  for (const key of ["oneOf", "anyOf"] as const) {
    if (Array.isArray(schema[key]) && schema[key].length > 0) {
      const variant =
        schema[key].find((s: JsonSchema) => s?.type !== "null" && s?.const !== null) ??
        schema[key][0];
      return schemaTemplate(variant, root, depth + 1);
    }
  }

  let type = schema.type;

  if (Array.isArray(type)) {
    type = type.find(t => t !== "null") ?? type[0];
  }

  if (!type && schema.properties) {
    type = "object";
  }

  switch (type) {
    case "object": {
      const value: Record<string, unknown> = {};
      const required: string[] = Array.isArray(schema.required) ? schema.required : [];
      for (const key of required) {
        value[key] = schemaTemplate(schema.properties?.[key] ?? {}, root, depth + 1);
      }
      return value;
    }
    case "array": {
      const minItems = typeof schema.minItems === "number" ? schema.minItems : 0;
      return Array.from({ length: minItems }, () =>
        schemaTemplate(schema.items ?? {}, root, depth + 1),
      );
    }
    case "integer":
    case "number":
      return typeof schema.minimum === "number" ? schema.minimum : 0;
    case "boolean":
      return false;
    case "string":
      return "";
    case "null":
      return null;
    default:
      return {};
  }
}

/**
 * Returns a pretty-printed template JSON for the given schema.
 */
export function schemaTemplateJson(schema: JsonSchema | undefined): string {
  return JSON.stringify(schema ? schemaTemplate(schema) : {}, null, 2);
}

export interface ValidationError {
  /** JSON pointer to the invalid value. */
  path: string;
  message: string;
}

export interface ValidationResult {
  valid: boolean;
  errors: ValidationError[];
}

function schemaDraft(schema: JsonSchema) {
  const uri: string = schema.$schema ?? "";

  if (uri.includes("draft-04")) return "4";
  if (uri.includes("draft-07") || uri.includes("draft-06")) return "7";
  if (uri.includes("2019-09")) return "2019-09";

  return "2020-12";
}

function leafErrors(errors: OutputUnit[]): ValidationError[] {
  // The validator reports a chain of errors from the root keyword
  // down to the failing one, we only care about the most specific ones.
  const specific = errors.filter(
    e =>
      !errors.some(
        other =>
          other !== e &&
          other.keywordLocation.startsWith(e.keywordLocation) &&
          other.keywordLocation.length > e.keywordLocation.length,
      ),
  );

  return specific.map(e => ({
    path: e.instanceLocation.replace(/^#/, "") || "/",
    message: e.error,
  }));
}

/**
 * Validates a JSON string against the given schema.
 *
 * Syntax errors are reported as validation errors as well.
 */
export function validateJson(text: string, schema?: JsonSchema): ValidationResult {
  let value: unknown;

  try {
    value = JSON.parse(text);
  } catch (error) {
    return {
      valid: false,
      errors: [{ path: "/", message: `Invalid JSON: ${(error as Error).message}` }],
    };
  }

  if (!schema) {
    return { valid: true, errors: [] };
  }

  try {
    const result = new Validator(schema as any, schemaDraft(schema), false).validate(value);
    return { valid: result.valid, errors: leafErrors(result.errors) };
  } catch (error) {
    // Invalid schemas should not block users.
    console.warn("failed to validate against schema", error);
    return { valid: true, errors: [] };
  }
}
