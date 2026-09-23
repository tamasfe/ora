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

/**
 * Returns a function that reports errors as toast messages,
 * aborted requests are ignored.
 */
export function useErrorToast() {
  const toast = useToast();

  return (error: unknown, summary = "Request failed") => {
    if (isAbortError(error)) {
      return;
    }

    console.error(error);
    toast.add({ severity: "error", summary, detail: errorMessage(error), life: 8000 });
  };
}
