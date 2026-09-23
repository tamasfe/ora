import type { MessageInitShape } from "@bufbuild/protobuf";
import { durationMs, type Duration } from "@bufbuild/protobuf/wkt";
import type { TimeRange } from "../api/ora/common/v1/time_range_pb";
import type { JobType } from "../api/ora/jobs/v1/job_pb";
import {
  MissedTimePolicy,
  type Schedule,
  type ScheduleSchema,
} from "../api/ora/schedules/v1/schedule_pb";
import {
  jobDraftFromJob,
  jobDraftProblems,
  jobDraftToJob,
  newJobDraft,
  type JobDraft,
} from "./job";
import { labelsToRows, rowsToLabels, type LabelRow } from "./labels";

/**
 * An editable schedule definition.
 */
export interface ScheduleDraft {
  policy: "interval" | "cron";
  interval?: Duration;
  cronExpression: string;
  immediate: boolean;
  missedTimePolicy: MissedTimePolicy;
  timeRange?: TimeRange;
  labels: LabelRow[];
  job: JobDraft;
}

export function newScheduleDraft(jobTypeId = ""): ScheduleDraft {
  return {
    policy: "interval",
    interval: undefined,
    cronExpression: "",
    immediate: false,
    missedTimePolicy: MissedTimePolicy.UNSPECIFIED,
    labels: [],
    job: newJobDraft(jobTypeId),
  };
}

/**
 * Creates a draft from an existing schedule definition (e.g. for cloning).
 */
export function scheduleDraftFromSchedule(schedule: Schedule): ScheduleDraft {
  const draft = newScheduleDraft();
  const policy = schedule.scheduling?.policy;

  if (policy?.case === "interval") {
    draft.policy = "interval";
    draft.interval = policy.value.interval;
    draft.immediate = policy.value.immediate;
    draft.missedTimePolicy = policy.value.missedTimePolicy;
  } else if (policy?.case === "cron") {
    draft.policy = "cron";
    draft.cronExpression = policy.value.cronExpression;
    draft.immediate = policy.value.immediate;
    draft.missedTimePolicy = policy.value.missedTimePolicy;
  }

  draft.timeRange = schedule.timeRange;
  draft.labels = labelsToRows(schedule.labels);

  if (schedule.jobTemplate) {
    draft.job = jobDraftFromJob(schedule.jobTemplate);
  }

  return draft;
}

export function scheduleDraftToSchedule(
  draft: ScheduleDraft,
): MessageInitShape<typeof ScheduleSchema> {
  const common = { immediate: draft.immediate, missedTimePolicy: draft.missedTimePolicy };

  return {
    scheduling: {
      policy:
        draft.policy === "interval"
          ? { case: "interval", value: { interval: draft.interval, ...common } }
          : { case: "cron", value: { cronExpression: draft.cronExpression.trim(), ...common } },
    },
    jobTemplate: jobDraftToJob({ ...draft.job, targetTimeMode: "now" }),
    labels: rowsToLabels(draft.labels),
    timeRange: draft.timeRange,
  };
}

/**
 * Returns the problems of a draft that prevent submitting it.
 */
export function scheduleDraftProblems(
  draft: ScheduleDraft,
  jobType: JobType | undefined,
): string[] {
  const problems: string[] = [];

  if (draft.policy === "interval" && (!draft.interval || durationMs(draft.interval) <= 0)) {
    problems.push("The interval must be greater than zero.");
  }

  if (draft.policy === "cron" && draft.cronExpression.trim() === "") {
    problems.push("A cron expression is required.");
  }

  if (draft.labels.some((label, i) => draft.labels.findIndex(l => l.key === label.key) !== i)) {
    problems.push("Label keys must be unique.");
  }

  problems.push(...jobDraftProblems(draft.job, jobType, true).map(p => `Job template: ${p}`));

  return problems;
}
