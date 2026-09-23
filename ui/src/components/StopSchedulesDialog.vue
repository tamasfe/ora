<script setup lang="ts">
import { ref, watch } from "vue";
import { useToast } from "primevue/usetoast";
import type { ScheduleFilters } from "../api/ora/admin/v1/schedules_pb";
import { useOraAdminClient } from "../grpc";
import { useErrorToast } from "../util/errors";

export interface StopSchedulesRequest {
  filters: ScheduleFilters;
  message: string;
}

/** The pending stop request, the dialog is visible while set. */
const request = defineModel<StopSchedulesRequest | undefined>();

const emit = defineEmits<{
  stopped: [scheduleIds: string[]];
}>();

const client = useOraAdminClient();
const toast = useToast();
const reportError = useErrorToast();

const cancelActiveJobs = ref(false);
const stopping = ref(false);

watch(request, () => (cancelActiveJobs.value = false));

async function stop() {
  if (!request.value) {
    return;
  }

  stopping.value = true;
  try {
    const res = await client.stopSchedules({
      filters: request.value.filters,
      cancelActiveJobs: cancelActiveJobs.value,
    });
    toast.add({
      severity: "success",
      summary: "Schedules stopped",
      detail: `${res.cancelledScheduleIds.length} schedule(s) stopped.`,
      life: 5000,
    });
    request.value = undefined;
    emit("stopped", res.cancelledScheduleIds);
  } catch (error) {
    reportError(error, "Failed to stop schedules");
  } finally {
    stopping.value = false;
  }
}
</script>

<template>
  <Dialog
    :visible="!!request"
    header="Stop schedules"
    modal
    class="w-full max-w-lg"
    @update:visible="!$event && (request = undefined)"
  >
    <div class="flex flex-col gap-4">
      <p>{{ request?.message }}</p>
      <Message severity="warn" size="small" :closable="false">
        Stopped schedules cannot be re-activated.
      </Message>
      <label class="flex items-center gap-2">
        <ToggleSwitch v-model="cancelActiveJobs" />
        <span>Also cancel active jobs of the stopped schedules</span>
      </label>
    </div>
    <template #footer>
      <Button label="Keep" severity="secondary" outlined @click="request = undefined" />
      <Button label="Stop schedules" severity="danger" :loading="stopping" @click="stop" />
    </template>
  </Dialog>
</template>
