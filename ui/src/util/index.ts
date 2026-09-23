import { onUnmounted, ref, shallowRef, watchEffect, type Ref } from "vue";
import { isAbortError, useErrorToast } from "./errors";

export interface AbortableFn {
  /**
   * Triggers the function to run,
   * aborting any previous runs if they are still in
   */
  run(): void;
  /**
   * Aborts any currently running function.
   */
  abort(): void;
}

/**
 * Runs the provided function with an AbortSignal that is automatically
 * aborted when the component is unmounted or when the dependencies change.
 */
export function useAbortable(f: (signal: AbortSignal) => unknown): AbortableFn {
  let controller = new AbortController();

  const run = () => {
    controller.abort();
    controller = new AbortController();
    f(controller.signal);
  };

  const abort = () => {
    controller.abort();
  };

  watchEffect(() => {
    run();
  });

  onUnmounted(() => {
    controller.abort();
  });

  return { run, abort };
}

export interface Loader<T> {
  /** The latest successfully loaded data. */
  data: Ref<T | undefined>;
  /** Whether a request is in progress. */
  loading: Ref<boolean>;
  /** The error of the latest request (if any). */
  error: Ref<unknown>;
  /** Reload the data. */
  reload(): void;
}

/**
 * Loads data with {@link useAbortable}, keeping track of the loading and error states.
 *
 * Errors are reported as toast messages.
 *
 * Just like with {@link useAbortable}, any reactive dependencies
 * that are accessed synchronously (before the first `await`) will trigger a reload.
 */
export function useLoader<T>(f: (signal: AbortSignal) => Promise<T>): Loader<T> {
  const data = shallowRef<T>();
  const loading = ref(false);
  const error = shallowRef<unknown>();
  const reportError = useErrorToast();

  const { run } = useAbortable(async signal => {
    loading.value = true;
    try {
      const promise = f(signal);
      const result = await promise;
      if (!signal.aborted) {
        data.value = result;
        error.value = undefined;
      }
    } catch (e) {
      if (!signal.aborted && !isAbortError(e)) {
        error.value = e;
        reportError(e);
      }
    } finally {
      if (!signal.aborted) {
        loading.value = false;
      }
    }
  });

  return { data, loading, error, reload: run };
}
