<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute } from "vue-router";
import { useInvalidQueryParams } from "./util/query";

const route = useRoute();

const items = [
  { label: "Dashboard", icon: "pi pi-home", route: "/" },
  { label: "Jobs", icon: "pi pi-briefcase", route: "/jobs" },
  { label: "Schedules", icon: "pi pi-calendar-clock", route: "/schedules" },
  { label: "Executors", icon: "pi pi-server", route: "/executors" },
  { label: "Job Types", icon: "pi pi-sitemap", route: "/job-types" },
  { label: "Maintenance", icon: "pi pi-wrench", route: "/maintenance" },
];

function isActive(path: string) {
  return path === "/" ? route.path === "/" : route.path.startsWith(path);
}

// A mistyped parameter (e.g. from an edited link) silently widens filters otherwise.
const invalidParams = useInvalidQueryParams();
const invalidKey = computed(() => invalidParams.value.join("&"));
const dismissedKey = ref<string>();
</script>

<template>
  <div class="min-h-screen">
    <Menubar :model="items" class="rounded-none! border-x-0! border-t-0! px-4!">
      <template #start>
        <RouterLink to="/" class="mr-4 flex items-center gap-2 text-lg font-semibold">
          <i class="pi pi-clock text-primary" />
          <span>Ora</span>
        </RouterLink>
      </template>
      <template #item="{ item, props }">
        <RouterLink v-slot="{ href, navigate }" :to="item.route" custom>
          <a
            :href="href"
            v-bind="props.action"
            :class="{ 'text-primary!': isActive(item.route) }"
            @click="navigate"
          >
            <span :class="item.icon" />
            <span>{{ item.label }}</span>
          </a>
        </RouterLink>
      </template>
    </Menubar>

    <main class="mx-auto max-w-screen-2xl p-4 md:p-6">
      <Message
        v-if="invalidParams.length > 0 && dismissedKey !== invalidKey"
        severity="warn"
        closable
        class="mb-4"
        @close="dismissedKey = invalidKey"
      >
        Some URL parameters are invalid and were ignored:
        <span class="font-mono">{{ invalidParams.join(", ") }}</span>
      </Message>
      <RouterView />
    </main>

    <Toast />
    <ConfirmDialog />
  </div>
</template>
