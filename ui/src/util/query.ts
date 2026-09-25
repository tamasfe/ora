import { create } from "@bufbuild/protobuf";
import { timestampDate, timestampFromDate, type Timestamp } from "@bufbuild/protobuf/wkt";
import {
  computed,
  onScopeDispose,
  shallowReactive,
  shallowRef,
  type ComputedRef,
  type Ref,
} from "vue";
import {
  useRoute,
  useRouter,
  type LocationQuery,
  type LocationQueryRaw,
  type LocationQueryValue,
  type RouteLocationNormalizedLoaded,
  type Router,
} from "vue-router";
import { LabelFilterSchema, type LabelFilter } from "../api/ora/common/v1/label_pb";
import { TimeRangeSchema, type TimeRange } from "../api/ora/common/v1/time_range_pb";
import { formatLabel, parseLabel } from "./labels";

/**
 * Converts a value from and to route query parameter values.
 *
 * A parameter can be repeated, so it has a list of values.
 */
export interface QueryParam<T> {
  /**
   * Parses the values of a parameter that is present in the query,
   * `undefined` means the value is invalid and the default is used.
   *
   * Invalid values are passed to `report` so that they can be shown to the user.
   */
  decode(values: string[], report: (invalid: string) => void): T | undefined;
  /** Encodes the value, an empty list removes the parameter. */
  encode(value: T): string[];
}

function formatIsoTimestamp(ts: Timestamp | undefined): string {
  // Whole seconds are shown without the fraction to keep the URLs short.
  return ts ? timestampDate(ts).toISOString().replace(".000Z", "Z") : "";
}

function parseIsoTimestamp(value: string): Timestamp | undefined | null {
  if (value === "") {
    return undefined;
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : timestampFromDate(date);
}

const stringParam: QueryParam<string> = {
  decode: values => values[0],
  encode: value => (value === "" ? [] : [value]),
};

const stringListParam: QueryParam<string[]> = {
  decode: values => values.filter(value => value !== ""),
  encode: values => values,
};

const timeRangeParam: QueryParam<TimeRange | undefined> = {
  decode(values, report) {
    const [start, end, ...rest] = values[0].split("..");
    const range = { start: parseIsoTimestamp(start), end: parseIsoTimestamp(end ?? "") };

    if (end === undefined || rest.length > 0 || range.start === null || range.end === null) {
      report(values[0]);
      return undefined;
    }

    return range.start || range.end
      ? create(TimeRangeSchema, { start: range.start, end: range.end })
      : undefined;
  },
  encode: range =>
    range?.start || range?.end
      ? [`${formatIsoTimestamp(range.start)}..${formatIsoTimestamp(range.end)}`]
      : [],
};

const labelFiltersParam: QueryParam<LabelFilter[]> = {
  decode(values, report) {
    const result: LabelFilter[] = [];
    for (const value of values) {
      const label = parseLabel(value);
      if (!label) {
        report(value);
        continue;
      }
      result.push(
        create(LabelFilterSchema, {
          key: label.key,
          value: label.value === "" ? undefined : label.value,
        }),
      );
    }
    return result;
  },
  encode: filters => filters.map(formatLabel),
};

/** Codecs for common parameter types. */
export const param = {
  string: stringParam,

  /** A list of strings, each one in a separate repeated parameter. */
  stringList: stringListParam,

  /** An integer, optionally restricted to the given values. */
  int(allowed?: readonly number[]): QueryParam<number> {
    return {
      decode(values, report) {
        const value = Number(values[0]);
        if (values[0] === "" || !Number.isInteger(value) || (allowed && !allowed.includes(value))) {
          report(values[0]);
          return undefined;
        }
        return value;
      },
      encode: value => [String(value)],
    };
  },

  /** An optional integer, `undefined` removes the parameter. */
  optionalInt: {
    decode(values, report) {
      const value = Number(values[0]);
      if (values[0] === "" || !Number.isInteger(value)) {
        report(values[0]);
        return undefined;
      }
      return value;
    },
    encode: value => (value === undefined ? [] : [String(value)]),
  } as QueryParam<number | undefined>,

  /** One of the given values by name, values without a name are omitted. */
  oneOf<T>(entries: Record<string, T>): QueryParam<T> {
    return {
      decode(values, report) {
        if (!Object.hasOwn(entries, values[0])) {
          report(values[0]);
          return undefined;
        }
        return entries[values[0]];
      },
      encode(value) {
        const name = Object.keys(entries).find(name => entries[name] === value);
        return name === undefined ? [] : [name];
      },
    };
  },

  /** A list of the given values by name, unknown names are dropped. */
  oneOfList<T>(entries: Record<string, T>): QueryParam<T[]> {
    return {
      decode(values, report) {
        const result: T[] = [];
        for (const value of values) {
          if (!Object.hasOwn(entries, value)) {
            report(value);
          } else if (!result.includes(entries[value])) {
            result.push(entries[value]);
          }
        }
        return result;
      },
      encode: list =>
        list.flatMap(value => Object.keys(entries).filter(name => entries[name] === value)),
    };
  },

  /** A time range as `start..end` in ISO 8601, either end can be omitted. */
  timeRange: timeRangeParam,

  /** Label filters as `key=value`, or just `key` to match any value. */
  labelFilters: labelFiltersParam,
};

/**
 * Maps the fields of an object to query parameters,
 * the names are prefixed when used, e.g. for tables embedded in pages.
 */
export type QueryFields<T> = {
  [K in keyof T]?: readonly [name: string, param: QueryParam<T[K]>];
};

type RawValue = LocationQueryValue | LocationQueryValue[] | undefined;

/** The values of a parameter, `?key` without a value is an empty value. */
function normalize(raw: RawValue): string[] {
  if (raw === undefined) {
    return [];
  }

  return (Array.isArray(raw) ? raw : [raw]).map(value => value ?? "");
}

function sameValues(a: string[], b: string[]) {
  return a.length === b.length && a.every((value, i) => value === b[i]);
}

/**
 * Query changes that are not applied to the route yet.
 *
 * Writes are batched and throttled, as browsers limit how often the history can be changed
 * (vue-router falls back to reloading the page if that fails), the overlay makes the new
 * values visible immediately and prevents writes in the same tick from overwriting each other.
 */
const overlay = shallowRef<{ path: string; query: LocationQuery } | null>(null);
/** Incremented on each write, so that settled flushes don't discard newer writes. */
let generation = 0;
let flushTimer: ReturnType<typeof setTimeout> | undefined;
const flushDelay = 150;
const installedRouters = new WeakSet<Router>();

function install(router: Router) {
  if (installedRouters.has(router)) {
    return;
  }
  installedRouters.add(router);

  router.afterEach((to, from) => {
    // Pending changes belong to the page that was left.
    if (to.path !== from.path) {
      overlay.value = null;
      clearTimeout(flushTimer);
      flushTimer = undefined;
    }
  });
}

function currentQuery(route: RouteLocationNormalizedLoaded): LocationQuery {
  const pending = overlay.value;
  return pending && pending.path === route.path ? pending.query : route.query;
}

function flush(router: Router) {
  flushTimer = undefined;
  const pending = overlay.value;

  if (!pending) {
    return;
  }

  if (router.currentRoute.value.path !== pending.path) {
    overlay.value = null;
    return;
  }

  const flushed = generation;
  router.replace({ query: pending.query }).finally(() => {
    if (flushed === generation && overlay.value === pending) {
      overlay.value = null;
    }
  });
}

/** Sets or removes (with an empty list) query parameters. */
function writeQuery(router: Router, changes: Record<string, string[]>) {
  const route = router.currentRoute.value;
  const base = currentQuery(route);
  const query: LocationQuery = { ...base };
  let changed = false;

  for (const [key, values] of Object.entries(changes)) {
    if (sameValues(normalize(base[key]), values)) {
      continue;
    }

    changed = true;
    if (values.length === 0) {
      delete query[key];
    } else {
      query[key] = values.length === 1 ? values[0] : values;
    }
  }

  if (!changed) {
    return;
  }

  overlay.value = { path: route.path, query };
  generation += 1;
  flushTimer ??= setTimeout(() => flush(router), flushDelay);
}

/** Invalid values of the query parameters in use, see {@link useInvalidQueryParams}. */
const invalidSources = shallowReactive(new Set<ComputedRef<string[]>>());

function trackInvalid(invalid: ComputedRef<string[]>) {
  invalidSources.add(invalid);
  onScopeDispose(() => invalidSources.delete(invalid));
}

/**
 * The query parameters of the current page that were ignored because of invalid values
 * (e.g. a mistyped status), as `key=value` strings.
 */
export function useInvalidQueryParams(): ComputedRef<string[]> {
  return computed(() => [...invalidSources].flatMap(source => source.value));
}

/**
 * Keeps a value in a route query parameter, so that the state survives reloads
 * and links to the current view can be shared.
 *
 * Values equal to the default are omitted from the URL.
 * Without a key, the value is kept in a local ref instead (e.g. for embedded components).
 */
export function useRouteQuery<T>(
  key: string | undefined,
  param: QueryParam<T>,
  defaultValue: T,
): Ref<T> {
  if (key === undefined) {
    return shallowRef(defaultValue) as Ref<T>;
  }

  const route = useRoute();
  const router = useRouter();
  install(router);

  // Decoded from a string so that unrelated query changes don't produce new values.
  const raw = computed(() => JSON.stringify(normalize(currentQuery(route)[key])));
  const defaultEncoded = param.encode(defaultValue);

  const value = computed(() => {
    const values: string[] = JSON.parse(raw.value);
    return values.length === 0 ? defaultValue : (param.decode(values, () => {}) ?? defaultValue);
  });

  trackInvalid(
    computed(() => {
      const invalid: string[] = [];
      const values: string[] = JSON.parse(raw.value);
      if (values.length > 0) {
        param.decode(values, value => invalid.push(`${key}=${value}`));
      }
      return invalid;
    }),
  );

  return computed({
    get: () => value.value,
    set(next) {
      const encoded = param.encode(next);
      writeQuery(router, { [key]: sameValues(encoded, defaultEncoded) ? [] : encoded });
    },
  });
}

/**
 * Keeps an object (e.g. filters) in route query parameters, one for each mapped field.
 *
 * Without a prefix, the value is kept in a local ref instead (e.g. for embedded components),
 * an empty prefix uses the field names as they are.
 */
export function useRouteQueryFields<T extends object>(
  prefix: string | undefined,
  fields: QueryFields<T>,
  init: () => T,
): Ref<T> {
  if (prefix === undefined) {
    return shallowRef(init()) as Ref<T>;
  }

  const route = useRoute();
  const router = useRouter();
  install(router);

  const entries = fieldEntries(fields);
  const defaults = encodeFields(entries, init());

  const raw = computed(() => {
    const query = currentQuery(route);
    return JSON.stringify(entries.map(([, name]) => normalize(query[prefix + name])));
  });

  const value = computed(() => decodeFields(entries, JSON.parse(raw.value), init, () => {}));

  trackInvalid(
    computed(() => {
      const invalid: string[] = [];
      decodeFields(entries, JSON.parse(raw.value), init, (name, value) =>
        invalid.push(`${prefix}${name}=${value}`),
      );
      return invalid;
    }),
  );

  return computed({
    get: () => value.value,
    set(next) {
      const encoded = encodeFields(entries, next);
      writeQuery(
        router,
        Object.fromEntries(
          entries.map(([, name], i) => [
            prefix + name,
            sameValues(encoded[i], defaults[i]) ? [] : encoded[i],
          ]),
        ),
      );
    },
  });
}

type FieldEntry = [field: string, name: string, param: QueryParam<unknown>];

function fieldEntries<T>(fields: QueryFields<T>): FieldEntry[] {
  return Object.entries(fields).map(([field, mapping]) => {
    const [name, param] = mapping as readonly [string, QueryParam<unknown>];
    return [field, name, param];
  });
}

function encodeFields(entries: FieldEntry[], value: object): string[][] {
  return entries.map(([field, , param]) => param.encode((value as any)[field]));
}

function decodeFields<T extends object>(
  entries: FieldEntry[],
  values: string[][],
  init: () => T,
  report: (name: string, value: string) => void,
): T {
  const result = init();

  entries.forEach(([field, name, param], i) => {
    if (values[i].length === 0) {
      return;
    }

    const decoded = param.decode(values[i], value => report(name, value));
    if (decoded !== undefined) {
      (result as any)[field] = decoded;
    }
  });

  return result;
}

/**
 * Encodes an object with the given field mapping, e.g. to create links to filtered views.
 */
export function encodeQueryFields<T extends object>(
  fields: QueryFields<T>,
  value: T,
  prefix = "",
): LocationQueryRaw {
  const query: LocationQueryRaw = {};

  for (const [field, name, param] of fieldEntries(fields)) {
    const values = param.encode((value as any)[field]);
    if (values.length > 0) {
      query[prefix + name] = values.length === 1 ? values[0] : values;
    }
  }

  return query;
}
