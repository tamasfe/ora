import { MissedTimePolicy, type SchedulingPolicy } from "../api/ora/schedules/v1/schedule_pb";
import { formatDuration } from "./format";

export const missedTimePolicyOptions = [
  { label: "Default", value: MissedTimePolicy.UNSPECIFIED, description: "Implementation defined" },
  { label: "Skip", value: MissedTimePolicy.SKIP, description: "Skip missed times" },
  {
    label: "Create",
    value: MissedTimePolicy.CREATE,
    description: "Create a job for each missed time",
  },
];

export function missedTimePolicyLabel(policy: MissedTimePolicy) {
  return missedTimePolicyOptions.find(o => o.value === policy)?.label ?? "Default";
}

/**
 * A short human-readable summary of a scheduling policy.
 */
export function schedulingSummary(policy?: SchedulingPolicy): string {
  switch (policy?.policy.case) {
    case "interval":
      return `Every ${formatDuration(policy.policy.value.interval)}`;
    case "cron":
      return `Cron: ${policy.policy.value.cronExpression}`;
    default:
      return "-";
  }
}
