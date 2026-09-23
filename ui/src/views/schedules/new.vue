<script setup lang="ts">
import { computed, ref, toRaw } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useToast } from "primevue/usetoast";
import { create, toJsonString } from "@bufbuild/protobuf";
import { AddSchedulesRequestSchema } from "../../api/ora/admin/v1/admin_pb";
import { ScheduleFiltersSchema, type ScheduleFilters } from "../../api/ora/admin/v1/schedules_pb";
import { useOraAdminClient } from "../../grpc";
import { useLoader } from "../../util";
import { useJobTypes } from "../../util/data";
import { useErrorToast } from "../../util/errors";
import { formatCount, formatLatency, formatSeconds } from "../../util/format";
import { rowsToLabelFilters } from "../../util/labels";
import { schedulesLink } from "../../util/route";
import {
  newScheduleDraft,
  scheduleDraftFromSchedule,
  scheduleDraftProblems,
  scheduleDraftToSchedule,
  type ScheduleDraft,
} from "../../util/scheduleDraft";
import { useDebounced, useStopwatch } from "../../util/time";

const route = useRoute();
const router = useRouter();
const client = useOraAdminClient();
const toast = useToast();
const reportError = useErrorToast();
const { byId } = useJobTypes();

const drafts = ref<ScheduleDraft[]>([
  newScheduleDraft(typeof route.query.jobType === "string" ? route.query.jobType : ""),
]);
const active = ref(0);

const cloneFrom = typeof route.query.from === "string" ? route.query.from : undefined;
const cloning = ref(!!cloneFrom);

if (cloneFrom) {
  client
    .listSchedules({ filters: { scheduleIds: [cloneFrom] }, pagination: { pageSize: 1 } })
    .then(res => {
      const schedule = res.schedules[0]?.schedule;
      if (schedule) {
        drafts.value = [scheduleDraftFromSchedule(schedule)];
      } else {
        toast.add({
          severity: "warn",
          summary: "Schedule not found",
          detail: cloneFrom,
          life: 5000,
        });
      }
    })
    .catch(error => reportError(error, "Failed to load schedule"))
    .finally(() => (cloning.value = false));
}

function addDraft() {
  drafts.value.push(newScheduleDraft(drafts.value[active.value]?.job.jobTypeId));
  active.value = drafts.value.length - 1;
}

function duplicateDraft() {
  drafts.value.push(structuredClone(toRaw(drafts.value[active.value])));
  active.value = drafts.value.length - 1;
}

function removeDraft(index: number) {
  drafts.value.splice(index, 1);
  active.value = Math.min(active.value, drafts.value.length - 1);
}

const problems = computed(() =>
  drafts.value.map(draft => scheduleDraftProblems(draft, byId.value.get(draft.job.jobTypeId))),
);
const valid = computed(() => problems.value.every(p => p.length === 0));

const inheritLabels = ref(true);
const useIfNotExists = ref(false);
const ifNotExists = ref(create(ScheduleFiltersSchema));

function matchLabelsOfDraft() {
  ifNotExists.value = {
    ...ifNotExists.value,
    labels: rowsToLabelFilters(drafts.value[active.value]?.labels ?? []),
  };
}

function hasFilters(filters: ScheduleFilters) {
  return toJsonString(ScheduleFiltersSchema, filters) !== "{}";
}

// Whether any schedule matches, checked with a single-item page as counting can be slow
// (and keeps running on the server even if the request is aborted).
const debouncedIfNotExists = useDebounced(ifNotExists, 400);
const matching = useLoader(
  () => (useIfNotExists.value ? debouncedIfNotExists.value : undefined),
  async (filters, signal) => {
    const res = await client.listSchedules({ filters, pagination: { pageSize: 1 } }, { signal });
    return res.schedules.length > 0;
  },
  { key: filters => toJsonString(ScheduleFiltersSchema, filters) },
);
const checkingMatches = computed(
  () =>
    !matching.error.value &&
    (matching.data.value === undefined ||
      matching.stale.value ||
      debouncedIfNotExists.value !== ifNotExists.value),
);

function buildRequest() {
  return create(AddSchedulesRequestSchema, {
    schedules: drafts.value.map(scheduleDraftToSchedule),
    ifNotExists: useIfNotExists.value ? ifNotExists.value : undefined,
    inheritLabels: inheritLabels.value,
  });
}

const preview = ref<string>();

function showPreview() {
  preview.value = toJsonString(AddSchedulesRequestSchema, buildRequest(), { prettySpaces: 2 });
}

/** The IDs of added schedules are put in the URL, above this all schedules are listed. */
const maxLinkedSchedules = 50;

const submitting = useStopwatch();
/** Schedules that prevented adding new ones, with the filters that matched them. */
const existing = ref<{ count: number; filters: ScheduleFilters }>();

async function submit() {
  existing.value = undefined;
  const request = buildRequest();

  try {
    const { result: res, ms } = await submitting.time(() => client.addSchedules(request));

    if (res.scheduleIds.length === 0) {
      existing.value = {
        count: res.existingScheduleIds.length,
        filters: request.ifNotExists ?? create(ScheduleFiltersSchema),
      };
      toast.add({
        severity: "info",
        summary: "No schedules added",
        detail: "Matching schedules already exist.",
        life: 5000,
      });
      return;
    }

    toast.add({
      severity: "success",
      summary: "Schedules added",
      detail: `${formatCount(res.scheduleIds.length)} schedule(s) added in ${formatLatency(ms)}.`,
      life: 5000,
    });

    if (res.scheduleIds.length === 1) {
      router.push(`/schedules/${res.scheduleIds[0]}`);
    } else if (res.scheduleIds.length <= maxLinkedSchedules) {
      router.push(schedulesLink({ scheduleIds: res.scheduleIds }));
    } else {
      router.push("/schedules");
    }
  } catch (error) {
    reportError(error, "Failed to add schedules");
  }
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex items-center gap-2">
      <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
      <h1 class="text-2xl font-semibold">New schedule{{ drafts.length > 1 ? "s" : "" }}</h1>
    </div>

    <div class="grid grid-cols-1 gap-4 lg:grid-cols-3">
      <Card class="lg:col-span-2">
        <template #content>
          <div v-if="cloning" class="flex flex-col gap-5" aria-busy="true">
            <Skeleton height="2.5rem" />
            <Skeleton height="6rem" />
            <Skeleton height="10rem" />
          </div>
          <Tabs v-else v-model:value="active">
            <TabList v-if="drafts.length > 1">
              <Tab v-for="(_, index) in drafts" :key="index" :value="index">
                <span class="flex items-center gap-2">
                  <i
                    v-if="problems[index].length > 0"
                    class="pi pi-exclamation-circle text-red-500"
                  />
                  Schedule {{ index + 1 }}
                </span>
              </Tab>
            </TabList>
            <TabPanels class="px-0!">
              <TabPanel v-for="(_, index) in drafts" :key="index" :value="index">
                <ScheduleDefinitionForm v-model="drafts[index]" />
              </TabPanel>
            </TabPanels>
          </Tabs>
          <div class="mt-4 flex flex-wrap gap-2">
            <Button
              label="Add another schedule"
              icon="pi pi-plus"
              severity="secondary"
              outlined
              size="small"
              @click="addDraft"
            />
            <Button
              label="Duplicate"
              icon="pi pi-copy"
              severity="secondary"
              outlined
              size="small"
              @click="duplicateDraft"
            />
            <Button
              v-if="drafts.length > 1"
              :label="`Remove schedule ${active + 1}`"
              icon="pi pi-trash"
              severity="danger"
              text
              size="small"
              @click="removeDraft(active)"
            />
          </div>
        </template>
      </Card>

      <div class="flex flex-col gap-4">
        <Card>
          <template #title>Submit</template>
          <template #content>
            <div class="flex flex-col gap-3">
              <label class="flex items-center gap-2">
                <ToggleSwitch v-model="inheritLabels" />
                <span>Jobs inherit schedule labels</span>
              </label>

              <Message v-if="valid" severity="success" size="small" variant="simple">
                {{ drafts.length }} schedule(s) ready to be added.
              </Message>
              <template v-for="(list, index) in problems" :key="index">
                <Message
                  v-for="problem in list"
                  :key="problem"
                  severity="error"
                  size="small"
                  variant="simple"
                >
                  <span v-if="drafts.length > 1" class="font-medium"
                    >Schedule {{ index + 1 }}: </span
                  >{{ problem }}
                </Message>
              </template>

              <Message v-if="existing" severity="info" size="small">
                {{ formatCount(existing.count) }} matching schedule{{
                  existing.count === 1 ? "" : "s"
                }}
                already exist{{ existing.count === 1 ? "s" : "" }}, no schedules were added.
                <RouterLink
                  :to="schedulesLink(existing.filters)"
                  target="_blank"
                  class="block text-primary hover:underline"
                >
                  View matching schedules <i class="pi pi-external-link text-xs" />
                </RouterLink>
              </Message>

              <div class="flex gap-2">
                <Button
                  :label="
                    (submitting.elapsed.value ?? 0) >= 1000
                      ? `Adding… ${formatSeconds(submitting.elapsed.value ?? 0)}`
                      : drafts.length > 1
                        ? `Add ${drafts.length} schedules`
                        : 'Add schedule'
                  "
                  icon="pi pi-check"
                  :disabled="!valid || cloning"
                  :loading="submitting.running.value"
                  class="flex-1"
                  @click="submit"
                />
                <Button
                  v-tooltip.bottom="'Preview request'"
                  icon="pi pi-code"
                  severity="secondary"
                  outlined
                  aria-label="Preview request"
                  @click="showPreview"
                />
              </div>
            </div>
          </template>
        </Card>

        <Card>
          <template #title>
            <label class="flex items-center gap-2 text-base">
              <ToggleSwitch v-model="useIfNotExists" />
              Only add if no matching schedule exists
            </label>
          </template>
          <template #content>
            <div v-if="useIfNotExists" class="flex flex-col gap-3">
              <small class="text-muted-color">
                If any schedule matches these filters, no schedules are added.
              </small>
              <div>
                <Button
                  label="Match labels of the current schedule"
                  icon="pi pi-tags"
                  severity="secondary"
                  outlined
                  size="small"
                  :disabled="(drafts[active]?.labels.length ?? 0) === 0"
                  @click="matchLabelsOfDraft"
                />
              </div>
              <ScheduleFilters v-model="ifNotExists" />

              <Message
                v-if="!hasFilters(ifNotExists)"
                severity="warn"
                size="small"
                variant="simple"
              >
                No filters are set, any existing schedule prevents adding.
              </Message>
              <Message v-if="checkingMatches" severity="secondary" size="small" variant="simple">
                <i class="pi pi-spin pi-spinner mr-1 text-xs" />Checking for matching schedules…
              </Message>
              <Message
                v-else-if="matching.error.value"
                severity="error"
                size="small"
                variant="simple"
              >
                Matching schedules could not be checked.
              </Message>
              <Message
                v-else-if="matching.data.value"
                severity="warn"
                size="small"
                variant="simple"
              >
                A matching schedule exists, nothing would be added.
                <RouterLink
                  :to="schedulesLink(ifNotExists)"
                  target="_blank"
                  class="text-primary hover:underline"
                >
                  View matching schedules <i class="pi pi-external-link text-xs" />
                </RouterLink>
              </Message>
              <Message v-else severity="success" size="small" variant="simple">
                No schedule matches, the schedules would be added.
              </Message>
            </div>
            <small v-else class="text-muted-color">
              Prevent duplicates, e.g. by checking for active schedules with the same labels.
            </small>
          </template>
        </Card>
      </div>
    </div>

    <Dialog
      :visible="preview !== undefined"
      header="AddSchedules request"
      modal
      class="w-full max-w-3xl"
      @update:visible="!$event && (preview = undefined)"
    >
      <JsonEditor :model-value="preview" readonly />
    </Dialog>
  </div>
</template>
