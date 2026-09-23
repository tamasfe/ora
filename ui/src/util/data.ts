import { computed, ref, shallowRef } from "vue";
import type { Client } from "@connectrpc/connect";
import type { AdminService } from "../api/ora/admin/v1/admin_pb";
import type { Executor } from "../api/ora/admin/v1/executors_pb";
import type { JobType } from "../api/ora/jobs/v1/job_pb";
import { useOraAdminClient } from "../grpc";
import { useErrorToast } from "./errors";

/**
 * Creates a shared, cached resource that is loaded on first use
 * and can be refreshed by any consumer.
 */
function sharedResource<T>(
  initial: T,
  load: (client: Client<typeof AdminService>) => Promise<T>,
  { reloadOnUse }: { reloadOnUse: boolean },
) {
  const data = shallowRef<T>(initial);
  const loading = ref(false);
  const loaded = ref(false);
  let pending: Promise<void> | undefined;

  return () => {
    const client = useOraAdminClient();
    const reportError = useErrorToast();

    const reload = () => {
      pending ??= load(client)
        .then(result => {
          data.value = result;
          loaded.value = true;
        })
        .catch(reportError)
        .finally(() => {
          loading.value = false;
          pending = undefined;
        });
      loading.value = true;
      return pending;
    };

    if (!loaded.value || reloadOnUse) {
      reload();
    }

    return { data, loading, loaded, reload };
  };
}

const useSharedJobTypes = sharedResource<JobType[]>(
  [],
  async client => (await client.listJobTypes({})).jobTypes,
  { reloadOnUse: false },
);

const useSharedExecutors = sharedResource<Executor[]>(
  [],
  async client => (await client.listExecutors({})).executors,
  { reloadOnUse: true },
);

/**
 * All job types known to the server, shared between components.
 */
export function useJobTypes() {
  const { data, ...rest } = useSharedJobTypes();

  const byId = computed(() => new Map(data.value.map(jobType => [jobType.id, jobType])));

  return { jobTypes: data, byId, ...rest };
}

/**
 * The currently connected executors, shared between components.
 *
 * The list is refreshed whenever a component starts using it.
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
