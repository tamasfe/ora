import type { Duration, Timestamp } from "@bufbuild/protobuf/wkt";
import { durationMs, timestampDate } from "@bufbuild/protobuf/wkt";

const dateTimeFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "medium",
});

const relativeFormat = new Intl.RelativeTimeFormat(undefined, {
  numeric: "auto",
});

const clockFormat = new Intl.DateTimeFormat(undefined, { timeStyle: "medium" });

const countFormat = new Intl.NumberFormat();

/**
 * Formats a timestamp as a local date and time.
 */
export function formatTimestamp(ts?: Timestamp): string {
  if (!ts) {
    return "-";
  }

  return dateTimeFormat.format(timestampDate(ts));
}

const relativeUnits: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 60 * 60 * 1000],
  ["month", 30 * 24 * 60 * 60 * 1000],
  ["day", 24 * 60 * 60 * 1000],
  ["hour", 60 * 60 * 1000],
  ["minute", 60 * 1000],
  ["second", 1000],
];

/**
 * Formats a timestamp relative to the current time, e.g. "5 minutes ago".
 */
export function formatRelative(ts?: Timestamp, now: number = Date.now()): string {
  if (!ts) {
    return "-";
  }

  const diff = timestampDate(ts).getTime() - now;

  for (const [unit, ms] of relativeUnits) {
    if (Math.abs(diff) >= ms || unit === "second") {
      return relativeFormat.format(Math.round(diff / ms), unit);
    }
  }

  return "-";
}

/**
 * Formats a duration in milliseconds in a human-readable way, e.g. "1h 2m 3s".
 */
export function formatMs(ms: number): string {
  if (ms < 1000) {
    return `${Math.round(ms)}ms`;
  }

  const parts: string[] = [];
  let rest = Math.round(ms / 1000);

  for (const [unit, secs] of [
    ["d", 86400],
    ["h", 3600],
    ["m", 60],
    ["s", 1],
  ] as const) {
    const value = Math.floor(rest / secs);
    rest -= value * secs;
    if (value > 0) {
      parts.push(`${value}${unit}`);
    }
  }

  return parts.join(" ");
}

/**
 * Formats the duration of a request, e.g. "45 ms" or "1.3 s".
 */
export function formatLatency(ms: number): string {
  if (ms < 1000) {
    return `${Math.round(ms)} ms`;
  }

  if (ms < 10_000) {
    return `${(ms / 1000).toFixed(1)} s`;
  }

  return formatSeconds(ms);
}

/**
 * Formats the time spent on an operation in progress in whole seconds, e.g. "12 s" or "2m 5s".
 */
export function formatSeconds(ms: number): string {
  return ms < 60_000 ? `${Math.floor(ms / 1000)} s` : formatMs(Math.floor(ms / 1000) * 1000);
}

/**
 * Formats a point in time (milliseconds since the epoch) as a local time of day.
 */
export function formatClock(ms: number): string {
  return clockFormat.format(ms);
}

/**
 * Formats a number with digit grouping, e.g. "12,345".
 */
export function formatCount(count: number): string {
  return countFormat.format(count);
}

/**
 * Formats a protobuf duration in a human-readable way.
 */
export function formatDuration(d?: Duration): string {
  if (!d) {
    return "-";
  }

  return formatMs(durationMs(d));
}

/**
 * Returns the time between two timestamps in a human-readable way.
 */
export function formatElapsed(start?: Timestamp, end?: Timestamp): string {
  if (!start || !end) {
    return "-";
  }

  return formatMs(timestampDate(end).getTime() - timestampDate(start).getTime());
}

/**
 * Pretty-prints a JSON string, returning the input as-is if it is not valid JSON.
 */
export function prettyJson(json?: string): string {
  if (json === undefined) {
    return "";
  }

  try {
    return JSON.stringify(JSON.parse(json), null, 2);
  } catch {
    return json;
  }
}

/**
 * Shortens an ID for display.
 *
 * IDs are UUIDv7s where the leading characters are a timestamp
 * that is shared by IDs created around the same time,
 * so the random trailing characters are shown instead.
 */
export function shortId(id: string): string {
  return id.length > 8 ? `…${id.slice(-8)}` : id;
}
