import { onUnmounted, watchEffect } from "vue";

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
