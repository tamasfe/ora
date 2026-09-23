import { ref, watch, type Ref } from "vue";

const prefix = "ora-ui:";

function read<T>(key: string, isValid: (value: unknown) => value is T): T | undefined {
  try {
    const raw = localStorage.getItem(prefix + key);
    if (raw === null) {
      return undefined;
    }
    const value: unknown = JSON.parse(raw);
    return isValid(value) ? value : undefined;
  } catch {
    // Storage might be unavailable (e.g. private mode) or the value corrupted.
    return undefined;
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(prefix + key, JSON.stringify(value));
  } catch {
    // Persisting is best-effort only.
  }
}

/**
 * A ref that is persisted in the browser's local storage.
 *
 * Invalid or missing stored values are replaced by the default value.
 */
export function useLocalStorageRef<T>(
  key: string,
  defaultValue: T,
  isValid: (value: unknown) => value is T,
): Ref<T> {
  const value = ref(read(key, isValid) ?? defaultValue) as Ref<T>;

  watch(value, v => write(key, v), { deep: true });

  return value;
}
