<script setup lang="ts">
import { computed, ref, toRaw } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useToast } from "primevue/usetoast";
import { create, toJsonString } from "@bufbuild/protobuf";
import { AddJobsRequestSchema, type AddJobsResponse } from "../../api/ora/admin/v1/admin_pb";
import { JobFiltersSchema } from "../../api/ora/admin/v1/jobs_pb";
import { useOraAdminClient } from "../../grpc";
import { useJobTypes } from "../../util/data";
import { useErrorToast } from "../../util/errors";
import {
  jobDraftFromJob,
  jobDraftProblems,
  jobDraftToJob,
  newJobDraft,
  type JobDraft,
} from "../../util/job";
import { rowsToLabelFilters } from "../../util/labels";
import { jobsLink } from "../../util/route";

const route = useRoute();
const router = useRouter();
const client = useOraAdminClient();
const toast = useToast();
const reportError = useErrorToast();
const { byId } = useJobTypes();

const drafts = ref<JobDraft[]>([
  newJobDraft(typeof route.query.jobType === "string" ? route.query.jobType : ""),
]);
const active = ref(0);

const cloneFrom = typeof route.query.from === "string" ? route.query.from : undefined;
const cloning = ref(!!cloneFrom);

if (cloneFrom) {
  client
    .listJobs({ filters: { jobIds: [cloneFrom] }, pagination: { pageSize: 1 } })
    .then(res => {
      const job = res.jobs[0]?.job;
      if (job) {
        drafts.value = [jobDraftFromJob(job)];
      } else {
        toast.add({ severity: "warn", summary: "Job not found", detail: cloneFrom, life: 5000 });
      }
    })
    .catch(error => reportError(error, "Failed to load job"))
    .finally(() => (cloning.value = false));
}

function addDraft() {
  drafts.value.push(newJobDraft(drafts.value[active.value]?.jobTypeId));
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
  drafts.value.map(draft => jobDraftProblems(draft, byId.value.get(draft.jobTypeId))),
);
const valid = computed(() => problems.value.every(p => p.length === 0));

const useIfNotExists = ref(false);
const ifNotExists = ref(create(JobFiltersSchema));

function matchLabelsOfDraft() {
  ifNotExists.value = {
    ...ifNotExists.value,
    labels: rowsToLabelFilters(drafts.value[active.value]?.labels ?? []),
  };
}

function buildRequest() {
  return create(AddJobsRequestSchema, {
    jobs: drafts.value.map(draft => jobDraftToJob(draft)),
    ifNotExists: useIfNotExists.value ? ifNotExists.value : undefined,
  });
}

const preview = ref<string>();

function showPreview() {
  preview.value = toJsonString(AddJobsRequestSchema, buildRequest(), { prettySpaces: 2 });
}

const submitting = ref(false);
const result = ref<AddJobsResponse>();

async function submit() {
  submitting.value = true;
  result.value = undefined;

  try {
    const res = await client.addJobs(buildRequest());

    if (res.jobIds.length === 0) {
      result.value = res;
      toast.add({
        severity: "info",
        summary: "No jobs added",
        detail: "Matching jobs already exist.",
        life: 5000,
      });
      return;
    }

    toast.add({
      severity: "success",
      summary: "Jobs added",
      detail: `${res.jobIds.length} job(s) added.`,
      life: 5000,
    });

    if (res.jobIds.length === 1) {
      router.push(`/jobs/${res.jobIds[0]}`);
    } else {
      router.push(jobsLink({ jobIds: res.jobIds }));
    }
  } catch (error) {
    reportError(error, "Failed to add jobs");
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex items-center gap-2">
      <Button icon="pi pi-arrow-left" severity="secondary" text rounded @click="router.back()" />
      <h1 class="text-2xl font-semibold">New job{{ drafts.length > 1 ? "s" : "" }}</h1>
    </div>

    <div class="grid grid-cols-1 gap-4 lg:grid-cols-3">
      <Card class="lg:col-span-2">
        <template #content>
          <ProgressBar v-if="cloning" mode="indeterminate" class="h-1!" />
          <Tabs v-else v-model:value="active">
            <TabList v-if="drafts.length > 1">
              <Tab v-for="(_, index) in drafts" :key="index" :value="index">
                <span class="flex items-center gap-2">
                  <i
                    v-if="problems[index].length > 0"
                    class="pi pi-exclamation-circle text-red-500"
                  />
                  Job {{ index + 1 }}
                </span>
              </Tab>
            </TabList>
            <TabPanels class="px-0!">
              <TabPanel v-for="(_, index) in drafts" :key="index" :value="index">
                <JobDefinitionForm v-model="drafts[index]" />
              </TabPanel>
            </TabPanels>
          </Tabs>
          <div class="mt-4 flex flex-wrap gap-2">
            <Button
              label="Add another job"
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
              :label="`Remove job ${active + 1}`"
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
              <Message v-if="valid" severity="success" size="small" variant="simple">
                {{ drafts.length }} job(s) ready to be added.
              </Message>
              <template v-for="(list, index) in problems" :key="index">
                <Message
                  v-for="problem in list"
                  :key="problem"
                  severity="error"
                  size="small"
                  variant="simple"
                >
                  <span v-if="drafts.length > 1" class="font-medium">Job {{ index + 1 }}: </span
                  >{{ problem }}
                </Message>
              </template>

              <Message
                v-if="result && result.existingJobIds.length > 0"
                severity="info"
                size="small"
              >
                Matching jobs already exist:
                <RouterLink
                  v-for="id in result.existingJobIds"
                  :key="id"
                  :to="`/jobs/${id}`"
                  class="block font-mono text-primary hover:underline"
                >
                  {{ id }}
                </RouterLink>
              </Message>

              <div class="flex gap-2">
                <Button
                  :label="drafts.length > 1 ? `Add ${drafts.length} jobs` : 'Add job'"
                  icon="pi pi-check"
                  :disabled="!valid || cloning"
                  :loading="submitting"
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
              Only add if no matching job exists
            </label>
          </template>
          <template #content>
            <div v-if="useIfNotExists" class="flex flex-col gap-3">
              <small class="text-muted-color">
                If any job matches these filters, no jobs are added.
              </small>
              <JobFilters v-model="ifNotExists" />
              <div>
                <Button
                  label="Match labels of current job"
                  icon="pi pi-tags"
                  severity="secondary"
                  text
                  size="small"
                  :disabled="(drafts[active]?.labels.length ?? 0) === 0"
                  @click="matchLabelsOfDraft"
                />
              </div>
            </div>
            <small v-else class="text-muted-color">
              Prevent duplicates, e.g. by checking for active jobs with the same labels.
            </small>
          </template>
        </Card>
      </div>
    </div>

    <Dialog
      :visible="preview !== undefined"
      header="AddJobs request"
      modal
      class="w-full max-w-3xl"
      @update:visible="!$event && (preview = undefined)"
    >
      <JsonEditor :model-value="preview" readonly />
    </Dialog>
  </div>
</template>
