import type { MessageInitShape } from "@bufbuild/protobuf";
import {
  durationMs,
  timestampDate,
  timestampFromDate,
  timestampNow,
  type Duration,
} from "@bufbuild/protobuf/wkt";
import {
  BackoffStrategy,
  TimeoutBaseTime,
  type Job,
  type JobSchema,
  type JobType,
  type TimeoutPolicy,
} from "../api/ora/jobs/v1/job_pb";
import { prettyJson } from "./format";
import { labelsToRows, rowsToLabels, type LabelRow } from "./labels";
import { parseSchema, schemaTemplateJson, validateJson } from "./schema";

export const timeoutBaseTimeOptions = [
  { label: "Default", value: TimeoutBaseTime.UNSPECIFIED },
  { label: "Target execution time", value: TimeoutBaseTime.TARGET_EXECUTION_TIME },
  { label: "Start time", value: TimeoutBaseTime.START_TIME },
];

export const backoffStrategyOptions = [
  { label: "Default", value: BackoffStrategy.UNSPECIFIED },
  { label: "Fixed", value: BackoffStrategy.FIXED },
  { label: "Exponential", value: BackoffStrategy.EXPONENTIAL },
];

export function timeoutBaseTimeLabel(value: TimeoutBaseTime) {
  return timeoutBaseTimeOptions.find(o => o.value === value)?.label ?? "Default";
}

export function backoffStrategyLabel(value: BackoffStrategy) {
  return backoffStrategyOptions.find(o => o.value === value)?.label ?? "Default";
}

/**
 * Whether the timeout policy has an actual timeout,
 * a zero timeout means no timeout.
 */
export function hasTimeout(policy?: TimeoutPolicy): boolean {
  return !!policy?.timeout && durationMs(policy.timeout) > 0;
}

/**
 * An editable job definition.
 */
export interface JobDraft {
  jobTypeId: string;
  /** The input payload JSON. */
  payload: string;
  /** Execute as soon as possible or at the given time. */
  targetTimeMode: "now" | "at";
  targetTime: Date | null;
  labels: LabelRow[];
  timeoutEnabled: boolean;
  timeout?: Duration;
  timeoutBaseTime: TimeoutBaseTime;
  retries: number;
  backoff?: Duration;
  maxBackoff?: Duration;
  backoffStrategy: BackoffStrategy;
  /** Jobs with higher priority are executed first. */
  priority: number;
}

export function newJobDraft(jobTypeId = ""): JobDraft {
  return {
    jobTypeId,
    payload: "{}",
    targetTimeMode: "now",
    targetTime: null,
    labels: [],
    timeoutEnabled: false,
    timeoutBaseTime: TimeoutBaseTime.UNSPECIFIED,
    retries: 0,
    backoffStrategy: BackoffStrategy.UNSPECIFIED,
    priority: 0,
  };
}

/**
 * Creates a draft from an existing job definition (e.g. for cloning).
 */
export function jobDraftFromJob(job: Job): JobDraft {
  const targetTime = job.targetExecutionTime ? timestampDate(job.targetExecutionTime) : null;

  return {
    jobTypeId: job.jobTypeId,
    payload: prettyJson(job.inputPayloadJson),
    // Past target times are replaced by "now".
    targetTimeMode: targetTime && targetTime.getTime() > Date.now() ? "at" : "now",
    targetTime,
    labels: labelsToRows(job.labels),
    timeoutEnabled: hasTimeout(job.timeoutPolicy),
    timeout: job.timeoutPolicy?.timeout,
    timeoutBaseTime: job.timeoutPolicy?.baseTime ?? TimeoutBaseTime.UNSPECIFIED,
    retries: Number(job.retryPolicy?.retries ?? 0n),
    backoff: job.retryPolicy?.backoffDuration,
    maxBackoff: job.retryPolicy?.maxBackoffDuration,
    backoffStrategy: job.retryPolicy?.backoffStrategy ?? BackoffStrategy.UNSPECIFIED,
    priority: job.priority,
  };
}

/**
 * Converts a draft to a job definition that can be sent to the server.
 *
 * The target time is required for schedule job templates as well,
 * the server replaces it with the scheduled times.
 */
export function jobDraftToJob(draft: JobDraft): MessageInitShape<typeof JobSchema> {
  let payload = draft.payload;
  try {
    // Send compact JSON.
    payload = JSON.stringify(JSON.parse(draft.payload));
  } catch {
    // Invalid JSON is caught by validation.
  }

  return {
    jobTypeId: draft.jobTypeId,
    inputPayloadJson: payload,
    targetExecutionTime:
      draft.targetTimeMode === "at" && draft.targetTime
        ? timestampFromDate(draft.targetTime)
        : timestampNow(),
    labels: rowsToLabels(draft.labels),
    timeoutPolicy:
      draft.timeoutEnabled && draft.timeout
        ? { timeout: draft.timeout, baseTime: draft.timeoutBaseTime }
        : undefined,
    retryPolicy: {
      retries: BigInt(draft.retries),
      backoffDuration: draft.backoff,
      backoffStrategy: draft.backoffStrategy,
      maxBackoffDuration:
        draft.backoffStrategy === BackoffStrategy.EXPONENTIAL ? draft.maxBackoff : undefined,
    },
    priority: draft.priority,
  };
}

/** The template payload for a job type. */
export function payloadTemplate(jobType?: JobType): string {
  return schemaTemplateJson(parseSchema(jobType?.inputSchemaJson));
}

/**
 * Returns the problems of a draft that prevent submitting it.
 *
 * @param isTemplate - The draft is a schedule job template, target time is not relevant.
 */
export function jobDraftProblems(
  draft: JobDraft,
  jobType: JobType | undefined,
  isTemplate = false,
): string[] {
  const problems: string[] = [];

  if (!draft.jobTypeId) {
    problems.push("A job type must be selected.");
    return problems;
  }

  if (!jobType) {
    problems.push(`Unknown job type "${draft.jobTypeId}".`);
  }

  const validation = validateJson(draft.payload, parseSchema(jobType?.inputSchemaJson));
  for (const error of validation.errors) {
    problems.push(`Input payload ${error.path}: ${error.message}`);
  }

  if (!isTemplate && draft.targetTimeMode === "at" && !draft.targetTime) {
    problems.push("A target execution time must be set.");
  }

  if (draft.timeoutEnabled && (!draft.timeout || durationMs(draft.timeout) <= 0)) {
    problems.push("The timeout must be greater than zero.");
  }

  if (draft.labels.some((label, i) => draft.labels.findIndex(l => l.key === label.key) !== i)) {
    problems.push("Label keys must be unique.");
  }

  return problems;
}
