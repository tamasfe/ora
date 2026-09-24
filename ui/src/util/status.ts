import { timestampMs } from "@bufbuild/protobuf/wkt";
import { ExecutionStatus, type Execution } from "../api/ora/admin/v1/executions_pb";
import type { Job } from "../api/ora/admin/v1/jobs_pb";
import { ScheduleStatus } from "../api/ora/admin/v1/schedules_pb";

export type Severity = "secondary" | "success" | "info" | "warn" | "danger" | "contrast";

export interface StatusInfo {
  label: string;
  severity: Severity;
  icon: string;
}

export const executionStatusInfo: Record<ExecutionStatus, StatusInfo> = {
  [ExecutionStatus.UNSPECIFIED]: {
    label: "Unknown",
    severity: "secondary",
    icon: "pi pi-question-circle",
  },
  [ExecutionStatus.PENDING]: { label: "Pending", severity: "info", icon: "pi pi-clock" },
  [ExecutionStatus.IN_PROGRESS]: {
    label: "In Progress",
    severity: "warn",
    icon: "pi pi-spin pi-spinner",
  },
  [ExecutionStatus.SUCCEEDED]: {
    label: "Succeeded",
    severity: "success",
    icon: "pi pi-check-circle",
  },
  [ExecutionStatus.FAILED]: { label: "Failed", severity: "danger", icon: "pi pi-times-circle" },
  [ExecutionStatus.CANCELLED]: { label: "Cancelled", severity: "secondary", icon: "pi pi-ban" },
};

/** Names of execution statuses in URLs. */
export const executionStatusNames: Record<string, ExecutionStatus> = {
  pending: ExecutionStatus.PENDING,
  in_progress: ExecutionStatus.IN_PROGRESS,
  succeeded: ExecutionStatus.SUCCEEDED,
  failed: ExecutionStatus.FAILED,
  cancelled: ExecutionStatus.CANCELLED,
};

/** Execution statuses that can be selected in filters. */
export const executionStatusOptions = [
  ExecutionStatus.PENDING,
  ExecutionStatus.IN_PROGRESS,
  ExecutionStatus.SUCCEEDED,
  ExecutionStatus.FAILED,
  ExecutionStatus.CANCELLED,
].map(value => ({ value, ...executionStatusInfo[value] }));

export const scheduleStatusInfo: Record<ScheduleStatus, StatusInfo> = {
  [ScheduleStatus.UNSPECIFIED]: {
    label: "Unknown",
    severity: "secondary",
    icon: "pi pi-question-circle",
  },
  [ScheduleStatus.ACTIVE]: { label: "Active", severity: "success", icon: "pi pi-play-circle" },
  [ScheduleStatus.STOPPED]: { label: "Stopped", severity: "secondary", icon: "pi pi-stop-circle" },
};

/** Names of schedule statuses in URLs. */
export const scheduleStatusNames: Record<string, ScheduleStatus> = {
  active: ScheduleStatus.ACTIVE,
  stopped: ScheduleStatus.STOPPED,
};

/** Schedule statuses that can be selected in filters. */
export const scheduleStatusOptions = [ScheduleStatus.ACTIVE, ScheduleStatus.STOPPED].map(value => ({
  value,
  ...scheduleStatusInfo[value],
}));

/**
 * Returns the latest execution of a job (if any).
 */
export function lastExecution(job: Job): Execution | undefined {
  let last: Execution | undefined;

  for (const execution of job.executions) {
    if (
      !last ||
      (execution.createdAt &&
        last.createdAt &&
        timestampMs(execution.createdAt) >= timestampMs(last.createdAt))
    ) {
      last = execution;
    }
  }

  return last;
}

/**
 * The status of a job is the status of its latest execution.
 */
export function jobStatus(job: Job): ExecutionStatus {
  return lastExecution(job)?.status ?? ExecutionStatus.UNSPECIFIED;
}

/**
 * Whether the job is still active (might be executed in the future).
 */
export function isJobActive(job: Job): boolean {
  const status = jobStatus(job);
  return (
    status === ExecutionStatus.PENDING ||
    status === ExecutionStatus.IN_PROGRESS ||
    status === ExecutionStatus.UNSPECIFIED
  );
}

/**
 * Returns the time an execution has finished (if it has).
 */
export function executionEndedAt(execution: Execution) {
  return execution.succeededAt ?? execution.failedAt ?? execution.cancelledAt;
}

/**
 * Returns the time the first execution of a job has started (if any).
 */
export function jobStartedAt(job: Job) {
  let first: Execution["startedAt"];

  for (const execution of job.executions) {
    if (
      execution.startedAt &&
      (!first || timestampMs(execution.startedAt) < timestampMs(first))
    ) {
      first = execution.startedAt;
    }
  }

  return first;
}

/**
 * Returns the time a job has finished, the end of its latest execution
 * if the job is no longer active.
 */
export function jobEndedAt(job: Job) {
  const last = lastExecution(job);
  return last && !isJobActive(job) ? executionEndedAt(last) : undefined;
}
