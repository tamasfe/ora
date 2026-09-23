import { onUnmounted, watch, type Ref } from "vue";

export const refreshIntervalOptions = [
  { label: "Off", value: 0 },
  { label: "5s", value: 5000 },
  { label: "30s", value: 30000 },
];

/**
 * Repeatedly calls the given function while the interval is positive.
 */
export function usePolling(f: () => void, interval: Ref<number>) {
  let handle: ReturnType<typeof setInterval> | undefined;

  const stop = () => {
    if (handle !== undefined) {
      clearInterval(handle);
      handle = undefined;
    }
  };

  watch(
    interval,
    value => {
      stop();
      if (value > 0) {
        handle = setInterval(f, value);
      }
    },
    { immediate: true },
  );

  onUnmounted(stop);
}
