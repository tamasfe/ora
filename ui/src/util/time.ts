import {
  computed,
  onScopeDispose,
  readonly,
  ref,
  shallowRef,
  watch,
  type Ref,
  type WatchSource,
} from "vue";

interface Clock {
  now: Ref<number>;
  users: number;
  timer: ReturnType<typeof setInterval>;
}

/** Clocks by update interval, shared by all users of the same interval. */
const clocks = new Map<number, Clock>();

/**
 * The current time in milliseconds, updated every `interval` milliseconds.
 */
export function useNow(interval = 1000): Readonly<Ref<number>> {
  let clock = clocks.get(interval);

  if (!clock) {
    const now = ref(Date.now());
    clock = { now, users: 0, timer: setInterval(() => (now.value = Date.now()), interval) };
    clocks.set(interval, clock);
  }

  const used = clock;
  used.users += 1;

  onScopeDispose(() => {
    used.users -= 1;
    if (used.users === 0) {
      clearInterval(used.timer);
      clocks.delete(interval);
    }
  });

  return readonly(used.now);
}

let visible: Ref<boolean> | undefined;

/**
 * Whether the page is visible, e.g. to pause polling in background tabs.
 */
export function useDocumentVisible(): Readonly<Ref<boolean>> {
  if (!visible) {
    const value = ref(!document.hidden);
    document.addEventListener("visibilitychange", () => (value.value = !document.hidden));
    visible = value;
  }

  return readonly(visible);
}

/**
 * Follows the source once it stopped changing for the given time.
 */
export function useDebounced<T>(source: WatchSource<T>, delay: number): Readonly<Ref<T>> {
  const value = shallowRef<T>(typeof source === "function" ? source() : source.value);
  let timer: ReturnType<typeof setTimeout> | undefined;

  watch(source, next => {
    clearTimeout(timer);
    timer = setTimeout(() => (value.value = next), delay);
  });

  onScopeDispose(() => clearTimeout(timer));

  return value;
}

/**
 * Measures operations (e.g. submitting a form), the elapsed time
 * can be shown while they are in progress.
 */
export function useStopwatch() {
  const startedAt = ref<number>();
  const now = useNow(1000);

  return {
    /** Whether an operation is in progress. */
    running: computed(() => startedAt.value !== undefined),
    /** Milliseconds since the operation in progress started. */
    elapsed: computed(() =>
      startedAt.value === undefined ? undefined : Math.max(0, now.value - startedAt.value),
    ),
    /** Runs the operation, resolves with its result and duration in milliseconds. */
    async time<T>(operation: () => Promise<T>): Promise<{ result: T; ms: number }> {
      const begin = performance.now();
      startedAt.value = Date.now();
      try {
        const result = await operation();
        return { result, ms: performance.now() - begin };
      } finally {
        startedAt.value = undefined;
      }
    },
  };
}
