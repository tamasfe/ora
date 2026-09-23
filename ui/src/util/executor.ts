import type { Executor } from "../api/ora/admin/v1/executors_pb";

/**
 * Total active and maximum concurrent executions of an executor across all queues.
 */
export function executorLoad(executor: Executor) {
  let active = 0;
  let max = 0;

  for (const queue of executor.queues) {
    active += Number(queue.activeExecutions);
    max += Number(queue.maxConcurrentExecutions);
  }

  return { active, max, percent: max > 0 ? Math.round((active / max) * 100) : 0 };
}
