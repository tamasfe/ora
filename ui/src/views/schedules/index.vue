<template>
  <div class="flex flex-col gap-4">
    <div class="flex items-center justify-between">
      <h1 class="text-2xl font-semibold">Schedules</h1>
      <RouterLink v-slot="{ navigate }" to="/schedules/new" custom>
        <Button label="New schedule" icon="pi pi-plus" @click="navigate" />
      </RouterLink>
    </div>
    <SchedulesTable query-prefix="" />
  </div>
</template>
