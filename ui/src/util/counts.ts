import { computed, shallowRef, type Ref } from "vue";
import type { Client } from "@connectrpc/connect";
import type { AdminService } from "../api/ora/admin/v1/admin_pb";
import type { ExecutionStatus } from "../api/ora/admin/v1/executions_pb";
import { ScheduleStatus } from "../api/ora/admin/v1/schedules_pb";
import { useOraAdminClient } from "../grpc";
import { useLoader, type LoadState } from ".";
import { executionStatusOptions } from "./status";

/** Job counts by status. */
export type StatusCounts = ReadonlyMap<ExecutionStatus, number>;

/** The counts of a job type. */
export interface JobTypeCounts {
  /** The number of jobs by status. */
  jobs: StatusCounts;
  /** The number of active schedules. */
  activeSchedules: number;
}

/**
 * Returns a function that runs at most `max` tasks at a time,
 * the rest wait in the order they were started.
 */
export function concurrencyLimit(max: number) {
  let running = 0;
  const waiting: (() => void)[] = [];

  return async <T>(task: () => Promise<T>, signal?: AbortSignal): Promise<T> => {
    while (running >= max) {
      await new Promise<void>(resolve => waiting.push(resolve));
    }

    if (signal?.aborted) {
      // Pass the turn on to the next task.
      waiting.shift()?.();
      signal.throwIfAborted();
    }

    running += 1;
    try {
      return await task();
    } finally {
      running -= 1;
      waiting.shift()?.();
    }
  };
}

/**
 * Each job type needs a request for each status, so many job types
 * would send a lot of requests at once to the server.
 */
const limitCounts = concurrencyLimit(4);

async function countJobType(
  client: Client<typeof AdminService>,
  jobTypeId: string,
  signal: AbortSignal,
): Promise<JobTypeCounts> {
  const [jobs, activeSchedules] = await Promise.all([
    Promise.all(
      executionStatusOptions.map(async ({ value: status }) => {
        const { count } = await limitCounts(
          () =>
            client.countJobs(
              { filters: { jobTypeIds: [jobTypeId], executionStatuses: [status] } },
              { signal },
            ),
          signal,
        );
        return [status, Number(count)] as const;
      }),
    ),
    limitCounts(
      () =>
        client.countSchedules(
          { filters: { jobTypeIds: [jobTypeId], statuses: [ScheduleStatus.ACTIVE] } },
          { signal },
        ),
      signal,
    ),
  ]);

  return { jobs: new Map(jobs), activeSchedules: Number(activeSchedules.count) };
}

/**
 * Counts the jobs by status and the active schedules of the given job types.
 *
 * The counts of each job type are available as soon as they are loaded. When the job types
 * change (e.g. paging), only the ones that were not counted yet are loaded,
 * reloading (e.g. polling) counts all of them again.
 */
export function useJobTypeCounts(jobTypeIds: () => string[]) {
  const client = useOraAdminClient();
  const counts = shallowRef<ReadonlyMap<string, JobTypeCounts>>(new Map());
  let recount = true;

  const loader = useLoader(
    () => {
      const ids = jobTypeIds();
      return ids.length > 0 ? ids : undefined;
    },
    async (ids, signal) => {
      const load = recount ? ids : ids.filter(id => !counts.value.has(id));
      recount = false;

      await Promise.all(
        load.map(async id => {
          const result = await countJobType(client, id, signal);
          counts.value = new Map(counts.value).set(id, result);
        }),
      );
    },
  );

  const state: LoadState = {
    ...loader,
    reload(force) {
      recount = true;
      loader.reload(force);
    },
  };

  return { counts, state };
}

/** The shortest interval between polling counts of job types. */
const countsMinInterval = 30_000;

/**
 * The refresh interval for polling counts of job types, which take many requests,
 * so they are not polled more often than every 30 seconds.
 */
export function useCountsInterval(interval: Readonly<Ref<number>>): Readonly<Ref<number>> {
  return computed(() => (interval.value > 0 ? Math.max(interval.value, countsMinInterval) : 0));
}
