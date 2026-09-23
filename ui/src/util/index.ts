import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import { isAbortError, useErrorToast } from "./errors";

/**
 * How long a request has to be in progress to be indicated,
 * so that fast requests (e.g. background refreshes) don't cause flickering.
 */
const busyDelay = 300;

/**
 * The state of something loaded from the server,
 * used for loading indicators, timing information and polling.
 */
export interface LoadState {
  /** Whether there is anything to load, disabled loaders don't send requests. */
  enabled: Readonly<Ref<boolean>>;
  /** Whether a request is in progress. */
  loading: Readonly<Ref<boolean>>;
  /**
   * Whether a request has been in progress for a while,
   * used for indicators and disabling actions without flickering on fast requests.
   */
  busy: Readonly<Ref<boolean>>;
  /** The error of the latest request, cleared once a request succeeds. */
  error: Readonly<Ref<unknown>>;
  /** When the request in progress started (milliseconds since the epoch). */
  startedAt: Readonly<Ref<number | undefined>>;
  /** How long the latest successful request took (milliseconds). */
  duration: Readonly<Ref<number | undefined>>;
  /** When the latest successful request finished (milliseconds since the epoch). */
  loadedAt: Readonly<Ref<number | undefined>>;
  /** When the latest request finished, successfully or not (milliseconds since the epoch). */
  settledAt: Readonly<Ref<number | undefined>>;
  /** The number of consecutive failed requests. */
  failures: Readonly<Ref<number>>;
  /**
   * Loads the data again, does nothing if a request is already in progress
   * unless `force` is set (e.g. after changing the data, a request in progress might miss it).
   */
  reload(force?: boolean): void;
}

export interface Loader<T> extends LoadState {
  /** The latest successfully loaded data. */
  data: Readonly<Ref<T | undefined>>;
  /**
   * The data was loaded with different parameters than the current ones
   * (e.g. previous filters), and is about to be replaced.
   */
  stale: Readonly<Ref<boolean>>;
}

/** A JSON key for the value, supports bigints (e.g. in protobuf timestamps). */
export function stableKey(value: unknown): string {
  return JSON.stringify(value, (_, v) => (typeof v === "bigint" ? v.toString() : v)) ?? "";
}

/**
 * Tracks the timing and error state of requests,
 * shared by loaders and shared resources.
 */
export function createRequestState() {
  const loading = ref(false);
  const busy = ref(false);
  const error = shallowRef<unknown>();
  const startedAt = ref<number>();
  const duration = ref<number>();
  const loadedAt = ref<number>();
  const settledAt = ref<number>();
  const failures = ref(0);

  let busyTimer: ReturnType<typeof setTimeout> | undefined;
  let begin = 0;

  return {
    state: { loading, busy, error, startedAt, duration, loadedAt, settledAt, failures },

    /** A request started, possibly replacing one in progress (which keeps it busy). */
    started() {
      begin = performance.now();
      loading.value = true;
      startedAt.value = Date.now();
      if (!busy.value) {
        clearTimeout(busyTimer);
        busyTimer = setTimeout(() => (busy.value = true), busyDelay);
      }
    },

    succeeded() {
      error.value = undefined;
      failures.value = 0;
      duration.value = performance.now() - begin;
      loadedAt.value = Date.now();
    },

    /**
     * Returns whether this is the first of consecutive failures,
     * only those are reported, e.g. when polling.
     */
    failed(e: unknown): boolean {
      error.value = e;
      failures.value += 1;
      return failures.value === 1;
    },

    /** The request finished (`settled`) or was abandoned. */
    stopped(settled: boolean) {
      clearTimeout(busyTimer);
      loading.value = false;
      busy.value = false;
      startedAt.value = undefined;
      if (settled) {
        settledAt.value = Date.now();
      }
    },
  };
}

/**
 * Loads data depending on reactive parameters.
 *
 * `params` is tracked, a request is sent whenever its value (compared by `key`) changes,
 * aborting the previous one. If `params` returns `undefined`, the loader is disabled.
 * `load` is not tracked.
 *
 * Errors are reported as toast messages, once for consecutive failures.
 */
export function useLoader<P, T>(
  params: () => P | undefined,
  load: (params: P, signal: AbortSignal) => Promise<T>,
  { key = stableKey }: { key?: (params: P) => string } = {},
): Loader<T> {
  const data = shallowRef<T>();
  const loadedKey = ref<string>();
  const request = createRequestState();
  const reportError = useErrorToast({ deduplicate: true });

  const currentKey = computed(() => {
    const p = params();
    return p === undefined ? undefined : key(p);
  });

  let controller: AbortController | undefined;

  function abort() {
    if (controller) {
      controller.abort();
      controller = undefined;
      request.stopped(false);
    }
  }

  function run() {
    // Not tracked, only called from watch callbacks and event handlers.
    const p = params();
    if (p === undefined) {
      abort();
      return;
    }

    // A request in progress is replaced, the state stays loading.
    controller?.abort();

    const runKey = key(p);
    const current = new AbortController();
    controller = current;
    request.started();

    let promise: Promise<T>;
    try {
      promise = load(p, current.signal);
    } catch (e) {
      promise = Promise.reject(e);
    }

    promise
      .then(
        result => {
          if (!current.signal.aborted) {
            data.value = result;
            loadedKey.value = runKey;
            request.succeeded();
          }
        },
        e => {
          if (!current.signal.aborted && !isAbortError(e) && request.failed(e)) {
            reportError(e);
          }
        },
      )
      .finally(() => {
        if (controller === current) {
          controller = undefined;
          request.stopped(true);
        }
      });
  }

  watch(currentKey, next => (next === undefined ? abort() : run()), { immediate: true });
  onScopeDispose(abort);

  const { loading } = request.state;
  const enabled = computed(() => currentKey.value !== undefined);

  return {
    data,
    stale: computed(() => loadedKey.value !== undefined && loadedKey.value !== currentKey.value),
    enabled,
    ...request.state,
    reload(force = false) {
      if ((force || !loading.value) && enabled.value) {
        run();
      }
    },
  };
}
