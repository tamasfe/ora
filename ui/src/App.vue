<script setup lang="ts">
import { useRoute } from "vue-router";

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

    <main class="mx-auto max-w-7xl p-4 md:p-6">
      <RouterView />
    </main>

    <Toast />
    <ConfirmDialog />
  </div>
</template>
