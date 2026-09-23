import { Code, ConnectError } from "@connectrpc/connect";
import { useToast } from "primevue/usetoast";

/**
 * Whether the error was caused by an aborted request.
 */
export function isAbortError(error: unknown): boolean {
  if (error instanceof ConnectError) {
    return error.code === Code.Canceled;
  }

  return error instanceof DOMException && error.name === "AbortError";
}

/**
 * Returns a human-readable message for the error.
 */
export function errorMessage(error: unknown): string {
  if (error instanceof ConnectError) {
    if (error.rawMessage === "missing trailer") {
      // Error responses carry the status in headers that browsers hide by default.
      return "missing trailer, the server's CORS configuration might not expose the grpc-status and grpc-message headers";
    }

    return error.rawMessage || Code[error.code];
  }

  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}

/** Recently shown background errors, the same error is shown once in a while only. */
const recentErrors = new Map<string, number>();
const repeatAfter = 10_000;

/** Whether the error was shown recently, remembers it otherwise. */
function shownRecently(key: string): boolean {
  const now = Date.now();

  for (const [shown, at] of recentErrors) {
    if (at < now - repeatAfter) {
      recentErrors.delete(shown);
    }
  }

  if (recentErrors.has(key)) {
    return true;
  }

  recentErrors.set(key, now);
  return false;
}

/**
 * Returns a function that reports errors as toast messages,
 * aborted requests are ignored.
 *
 * With `deduplicate`, identical errors (e.g. from several background requests
 * failing during an outage) are only shown once within a few seconds,
 * errors of user actions should always be shown.
 */
export function useErrorToast({ deduplicate = false } = {}) {
  const toast = useToast();

  return (error: unknown, summary = "Request failed") => {
    if (isAbortError(error)) {
      return;
    }

    console.error(error);

    const detail = errorMessage(error);

    if (deduplicate && shownRecently(`${summary}\n${detail}`)) {
      return;
    }

    toast.add({ severity: "error", summary, detail, life: 8000 });
  };
}
