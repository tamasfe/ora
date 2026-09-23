<script setup lang="ts">
import { ref, watch } from "vue";
import { durationFromMs, durationMs, type Duration } from "@bufbuild/protobuf/wkt";

const props = defineProps<{
  placeholder?: string;
  inputId?: string;
}>();

const model = defineModel<Duration | undefined>();

const units = [
  { label: "ms", value: 1 },
  { label: "seconds", value: 1000 },
  { label: "minutes", value: 60 * 1000 },
  { label: "hours", value: 60 * 60 * 1000 },
  { label: "days", value: 24 * 60 * 60 * 1000 },
];

const amount = ref<number | null>(null);
const unit = ref(1000);

watch(
  model,
  duration => {
    if (!duration) {
      amount.value = null;
      return;
    }

    const ms = durationMs(duration);

    if (amount.value !== null && amount.value * unit.value === ms) {
      return;
    }

    // Pick the largest unit that represents the duration exactly.
    const best = [...units].reverse().find(u => ms % u.value === 0 && ms >= u.value) ?? units[0];
    unit.value = best.value;
    amount.value = ms / best.value;
  },
  { immediate: true },
);

function emit() {
  model.value = amount.value === null ? undefined : durationFromMs(amount.value * unit.value);
}
</script>

<template>
  <InputGroup>
    <InputNumber
      v-model="amount"
      :input-id="props.inputId"
      :min="0"
      :max-fraction-digits="3"
      :placeholder="props.placeholder"
      size="small"
      @update:model-value="emit"
    />
    <Select
      v-model="unit"
      :options="units"
      option-label="label"
      option-value="value"
      size="small"
      class="max-w-32"
      @update:model-value="emit"
    />
  </InputGroup>
</template>
