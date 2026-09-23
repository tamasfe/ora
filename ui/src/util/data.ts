import { computed, ref, shallowRef } from "vue";
import type { Client } from "@connectrpc/connect";
import type { AdminService } from "../api/ora/admin/v1/admin_pb";
import type { Executor } from "../api/ora/admin/v1/executors_pb";
import type { JobType } from "../api/ora/jobs/v1/job_pb";
import { useOraAdminClient } from "../grpc";
import { createRequestState, type LoadState } from ".";
import { useErrorToast } from "./errors";

/**
 * Creates a shared, cached resource that is loaded on first use
 * and can be refreshed by any consumer.
 *
 * The data is reloaded when a component starts using it if it is older than `maxAge`.
 */
function sharedResource<T>(
  initial: T,
  load: (client: Client<typeof AdminService>) => Promise<T>,
  { maxAge }: { maxAge: number },
) {
  const data = shallowRef<T>(initial);
  const loaded = ref(false);
  const request = createRequestState();
  const enabled = computed(() => true);
  let pending: Promise<void> | undefined;

  return () => {
    const client = useOraAdminClient();
    const reportError = useErrorToast({ deduplicate: true });

    const reload = () => {
      if (pending) {
        return;
      }

      request.started();
      pending = load(client)
        .then(
          result => {
            data.value = result;
            loaded.value = true;
            request.succeeded();
          },
          error => {
            if (request.failed(error)) {
              reportError(error);
            }
          },
        )
        .finally(() => {
          pending = undefined;
          request.stopped(true);
        });
    };

    const loadedAt = request.state.loadedAt.value;
    if (!loaded.value || (loadedAt !== undefined && Date.now() - loadedAt > maxAge)) {
      reload();
    }

    const state: LoadState = { enabled, ...request.state, reload };
    return { data, loaded, state, ...state };
  };
}

const useSharedJobTypes = sharedResource<JobType[]>(
  [],
  async client => (await client.listJobTypes({})).jobTypes,
  { maxAge: Infinity },
);

const useSharedExecutors = sharedResource<Executor[]>(
  [],
  async client => (await client.listExecutors({})).executors,
  { maxAge: 5000 },
);

/**
 * All job types known to the server, shared between components.
 *
 * The list is only loaded once, pages that list job types reload it explicitly.
 */
export function useJobTypes() {
  const { data, ...rest } = useSharedJobTypes();

  const byId = computed(() => new Map(data.value.map(jobType => [jobType.id, jobType])));

  return { jobTypes: data, byId, ...rest };
}

/**
 * The currently connected executors, shared between components.
 *
 * The list is refreshed when a component starts using it, unless it was loaded recently.
 */
export function useExecutors() {
  const { data, ...rest } = useSharedExecutors();

  /** Executors by the job type IDs they support. */
  const byJobType = computed(() => {
    const map = new Map<string, Executor[]>();
    for (const executor of data.value) {
      for (const queue of executor.queues) {
        const id = queue.jobType?.id;
        if (id) {
          map.set(id, [...(map.get(id) ?? []), executor]);
        }
      }
    }
    return map;
  });

  return { executors: data, byJobType, ...rest };
}
