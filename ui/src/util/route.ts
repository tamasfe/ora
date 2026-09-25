import { create, type MessageInitShape } from "@bufbuild/protobuf";
import { ref, watch, type Ref } from "vue";
import type { RouteLocationRaw } from "vue-router";
import { JobFiltersSchema, JobOrderBy, type JobFilters } from "../api/ora/admin/v1/jobs_pb";
import {
  ScheduleFiltersSchema,
  ScheduleOrderBy,
  type ScheduleFilters,
} from "../api/ora/admin/v1/schedules_pb";
import { encodeQueryFields, param, useRouteQuery, type QueryFields } from "./query";
import { executionStatusNames, scheduleStatusNames } from "./status";
import { useDebounced } from "./time";

/** Job filters in URLs, e.g. `?label=project=abc&status=failed`. */
export const jobFilterFields: QueryFields<JobFilters> = {
  labels: ["label", param.labelFilters],
  jobTypeIds: ["type", param.stringList],
  executionStatuses: ["status", param.oneOfList(executionStatusNames)],
  jobIds: ["id", param.stringList],
  scheduleIds: ["schedule", param.stringList],
  executorIds: ["executor", param.stringList],
  executionIds: ["execution", param.stringList],
  targetExecutionTime: ["target", param.timeRange],
  createdAt: ["created", param.timeRange],
  minPriority: ["min_priority", param.optionalInt],
  maxPriority: ["max_priority", param.optionalInt],
};

/** Schedule filters in URLs, e.g. `?label=project=abc&status=active`. */
export const scheduleFilterFields: QueryFields<ScheduleFilters> = {
  labels: ["label", param.labelFilters],
  jobTypeIds: ["type", param.stringList],
  statuses: ["status", param.oneOfList(scheduleStatusNames)],
  scheduleIds: ["id", param.stringList],
  createdAt: ["created", param.timeRange],
};

export const jobOrders: Record<string, JobOrderBy> = {
  created_desc: JobOrderBy.CREATED_AT_DESC,
  created_asc: JobOrderBy.CREATED_AT_ASC,
  target_desc: JobOrderBy.TARGET_EXECUTION_TIME_DESC,
  target_asc: JobOrderBy.TARGET_EXECUTION_TIME_ASC,
  priority_desc: JobOrderBy.PRIORITY_DESC,
  priority_asc: JobOrderBy.PRIORITY_ASC,
};

export const scheduleOrders: Record<string, ScheduleOrderBy> = {
  created_desc: ScheduleOrderBy.CREATED_AT_DESC,
  created_asc: ScheduleOrderBy.CREATED_AT_ASC,
};

/** Auto refresh intervals in URLs, off is the default. */
const refreshIntervals: Record<string, number> = {
  "5s": 5000,
  "30s": 30000,
};

/**
 * The name of a query parameter with the given prefix,
 * no prefix means the state is not kept in the URL.
 */
export function queryKey(prefix: string | undefined, name: string): string | undefined {
  return prefix === undefined ? undefined : prefix + name;
}

/**
 * The auto refresh interval in milliseconds kept in the URL (`?refresh=5s`), off by default.
 */
export function useRefreshInterval(key: string | undefined = "refresh"): Ref<number> {
  return useRouteQuery(key, param.oneOf(refreshIntervals), 0);
}

/**
 * The value of a search input kept in the URL (`?q=`),
 * the URL is updated once typing pauses.
 */
export function useSearchQuery(key = "q"): Ref<string> {
  const query = useRouteQuery(key, param.string, "");
  const input = ref(query.value);
  const debounced = useDebounced(input, 300);

  watch(debounced, value => (query.value = value));
  // Follow navigation, e.g. going back.
  watch(query, value => {
    if (value !== debounced.value) {
      input.value = value;
    }
  });

  return input;
}

/**
 * Link to the jobs page with the given filters.
 */
export function jobsLink(filters: MessageInitShape<typeof JobFiltersSchema>): RouteLocationRaw {
  return {
    path: "/jobs",
    query: encodeQueryFields(jobFilterFields, create(JobFiltersSchema, filters)),
  };
}

/**
 * Link to the schedules page with the given filters.
 */
export function schedulesLink(
  filters: MessageInitShape<typeof ScheduleFiltersSchema>,
): RouteLocationRaw {
  return {
    path: "/schedules",
    query: encodeQueryFields(scheduleFilterFields, create(ScheduleFiltersSchema, filters)),
  };
}
