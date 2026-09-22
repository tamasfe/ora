<script setup lang="ts">
import { ref } from "vue";
import { useOraAdminClient } from "../grpc";
import type { JobType } from "../api/ora/jobs/v1/job_pb";
import { useAbortable } from "../util";

const client = useOraAdminClient();

const jobTypes = ref<JobType[]>([]);

useAbortable(async signal => {
  const res = await client.listJobTypes(
    {},
    {
      signal,
    },
  );

  jobTypes.value = res.jobTypes;
});
</script>

<template>
  <div v-for="jobType in jobTypes">
    {{ jobType.id }}
  </div>
</template>
