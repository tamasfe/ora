import {
  create,
  fromJsonString,
  toJsonString,
  type DescMessage,
  type MessageInitShape,
  type MessageShape,
} from "@bufbuild/protobuf";
import { computed, type WritableComputedRef } from "vue";
import { useRoute, useRouter, type RouteLocationRaw } from "vue-router";
import { JobFiltersSchema } from "../api/ora/admin/v1/jobs_pb";
import { ScheduleFiltersSchema } from "../api/ora/admin/v1/schedules_pb";

function encodeMessage<Desc extends DescMessage>(
  schema: Desc,
  value: MessageInitShape<Desc>,
): string | undefined {
  const json = toJsonString(schema, create(schema, value));
  return json === "{}" ? undefined : json;
}

/**
 * Syncs a protobuf message (e.g. filters) with a route query parameter
 * in its JSON representation, so that the state is kept on reload
 * and links can be shared.
 */
export function useRouteQueryMessage<Desc extends DescMessage>(
  schema: Desc,
  key = "filters",
): WritableComputedRef<MessageShape<Desc>> {
  const route = useRoute();
  const router = useRouter();

  return computed({
    get() {
      const raw = route.query[key];

      if (typeof raw === "string") {
        try {
          return fromJsonString(schema, raw, { ignoreUnknownFields: true });
        } catch (error) {
          console.warn(`invalid "${key}" query parameter`, error);
        }
      }

      return create(schema);
    },
    set(value) {
      router.replace({
        query: { ...route.query, [key]: encodeMessage(schema, value) },
      });
    },
  });
}

/**
 * Link to the jobs page with the given filters.
 */
export function jobsLink(filters: MessageInitShape<typeof JobFiltersSchema>): RouteLocationRaw {
  return { path: "/jobs", query: { filters: encodeMessage(JobFiltersSchema, filters) } };
}

/**
 * Link to the schedules page with the given filters.
 */
export function schedulesLink(
  filters: MessageInitShape<typeof ScheduleFiltersSchema>,
): RouteLocationRaw {
  return { path: "/schedules", query: { filters: encodeMessage(ScheduleFiltersSchema, filters) } };
}
