<template>
  <div class="flex flex-col gap-4">
    <div class="flex items-center justify-between">
      <h1 class="text-2xl font-semibold">Jobs</h1>
      <RouterLink v-slot="{ navigate }" to="/jobs/new" custom>
        <Button label="New job" icon="pi pi-plus" @click="navigate" />
      </RouterLink>
    </div>
    <JobsTable query-prefix="" />
  </div>
</template>
