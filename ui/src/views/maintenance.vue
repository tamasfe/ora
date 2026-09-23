<script setup lang="ts">
import { ref } from "vue";
import { useConfirm } from "primevue/useconfirm";
import { useToast } from "primevue/usetoast";
import { timestampFromDate } from "@bufbuild/protobuf/wkt";
import { useOraAdminClient } from "../grpc";
import { useErrorToast } from "../util/errors";

const client = useOraAdminClient();
const confirm = useConfirm();
const toast = useToast();
const reportError = useErrorToast();

const day = 24 * 60 * 60 * 1000;

const presets = [
  { label: "1 day", value: 1 },
  { label: "7 days", value: 7 },
  { label: "30 days", value: 30 },
  { label: "90 days", value: 90 },
];

const preset = ref<number | null>(30);
const before = ref<Date>(new Date(Date.now() - 30 * day));
const deleting = ref(false);

function selectPreset(days: number | null) {
  if (days !== null) {
    before.value = new Date(Date.now() - days * day);
  }
}

function deleteData() {
  const cutoff = before.value;

  confirm.require({
    header: "Delete historical data",
    message: `Permanently delete inactive jobs, stopped schedules and unused job types from before ${cutoff.toLocaleString()}? This cannot be undone.`,
    icon: "pi pi-exclamation-triangle",
    rejectProps: { label: "Keep", severity: "secondary", outlined: true },
    acceptProps: { label: "Delete", severity: "danger" },
    accept: async () => {
      deleting.value = true;
      try {
        await client.deleteHistoricalData({ before: timestampFromDate(cutoff) });
        toast.add({ severity: "success", summary: "Historical data deleted", life: 5000 });
      } catch (error) {
        reportError(error, "Failed to delete historical data");
      } finally {
        deleting.value = false;
      }
    },
  });
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <h1 class="text-2xl font-semibold">Maintenance</h1>

    <Card class="max-w-2xl">
      <template #title>Delete historical data</template>
      <template #subtitle>Free up storage by removing data that is no longer needed.</template>
      <template #content>
        <div class="flex flex-col gap-4">
          <ul class="list-inside list-disc text-sm">
            <li>Inactive jobs (succeeded, failed or cancelled) and their executions</li>
            <li>Stopped schedules</li>
            <li>Job types that are no longer used</li>
          </ul>

          <div class="flex flex-col gap-1">
            <label class="font-medium">Delete data older than</label>
            <SelectButton
              v-model="preset"
              :options="presets"
              option-label="label"
              option-value="value"
              @update:model-value="selectPreset"
            />
          </div>

          <div class="flex flex-col gap-1">
            <label for="before" class="font-medium">Cutoff time</label>
            <DatePicker
              v-model="before"
              input-id="before"
              show-time
              hour-format="24"
              show-icon
              class="max-w-sm"
              @update:model-value="preset = null"
            />
          </div>

          <Message v-if="before.getTime() > Date.now()" severity="warn" size="small">
            The cutoff time is in the future, all inactive data will be deleted.
          </Message>

          <div>
            <Button
              label="Delete historical data"
              icon="pi pi-trash"
              severity="danger"
              :loading="deleting"
              @click="deleteData"
            />
          </div>
        </div>
      </template>
    </Card>
  </div>
</template>
