<script setup lang="ts">
import { ref, watch } from "vue";
import { useToast } from "primevue/usetoast";
import type { ScheduleFilters } from "../api/ora/admin/v1/schedules_pb";
import { useOraAdminClient } from "../grpc";
import { useErrorToast } from "../util/errors";
import { formatCount, formatLatency, formatSeconds } from "../util/format";
import { useStopwatch } from "../util/time";

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
const stopping = useStopwatch();

watch(request, () => (cancelActiveJobs.value = false));

async function stop() {
  if (!request.value) {
    return;
  }

  try {
    const filters = request.value.filters;
    const { result, ms } = await stopping.time(() =>
      client.stopSchedules({ filters, cancelActiveJobs: cancelActiveJobs.value }),
    );
    toast.add({
      severity: "success",
      summary: "Schedules stopped",
      detail: `${formatCount(result.cancelledScheduleIds.length)} schedule(s) stopped in ${formatLatency(ms)}.`,
      life: 5000,
    });
    request.value = undefined;
    emit("stopped", result.cancelledScheduleIds);
  } catch (error) {
    reportError(error, "Failed to stop schedules");
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
      <Button
        label="Keep"
        severity="secondary"
        outlined
        :disabled="stopping.running.value"
        @click="request = undefined"
      />
      <Button
        :label="
          (stopping.elapsed.value ?? 0) >= 1000
            ? `Stopping… ${formatSeconds(stopping.elapsed.value ?? 0)}`
            : 'Stop schedules'
        "
        severity="danger"
        :loading="stopping.running.value"
        @click="stop"
      />
    </template>
  </Dialog>
</template>
