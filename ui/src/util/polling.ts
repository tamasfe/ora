import { onScopeDispose, watch, type Ref } from "vue";
import type { LoadState } from ".";
import { useDocumentVisible } from "./time";

export const refreshIntervalOptions = [
  { label: "Off", value: 0 },
  { label: "5s", value: 5000 },
  { label: "30s", value: 30000 },
];

/** The longest delay between reloads after repeated failures. */
const maxBackoff = 60_000;

/**
 * Reloads the targets while the interval is positive.
 *
 * Each target is reloaded `interval` milliseconds after its previous request finished,
 * so slow requests are never interrupted and a slow target doesn't hold back the others.
 * Polling pauses while the page is hidden and backs off after failures.
 */
export function usePolling(targets: LoadState | LoadState[], interval: Readonly<Ref<number>>) {
  const visible = useDocumentVisible();

  for (const target of Array.isArray(targets) ? targets : [targets]) {
    let timer: ReturnType<typeof setTimeout> | undefined;

    watch(
      [interval, visible, target.enabled, target.loading, target.settledAt],
      ([ms, isVisible, enabled, loading, settledAt]) => {
        clearTimeout(timer);

        if (ms <= 0 || !isVisible || !enabled || loading) {
          return;
        }

        const delay = Math.min(ms * 2 ** target.failures.value, Math.max(ms, maxBackoff));
        const elapsed = settledAt === undefined ? delay : Date.now() - settledAt;
        timer = setTimeout(() => target.reload(), Math.max(0, delay - elapsed));
      },
      { immediate: true },
    );

    onScopeDispose(() => clearTimeout(timer));
  }
}
