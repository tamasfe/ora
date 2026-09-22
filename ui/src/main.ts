import { createApp } from "vue";
import "./style.css";
import App from "./App.vue";
import { createRouter, createWebHistory } from "vue-router";
import { handleHotUpdate, routes } from "vue-router/auto-routes";
import PrimeVue from "primevue/config";
import Aura from "@primeuix/themes/aura";
import { definePreset } from "@primeuix/themes";
import ConfirmationService from "primevue/confirmationservice";

import "primeicons/primeicons.css";
import { oraAdminClient } from "./grpc";

const router = createRouter({
  history: createWebHistory(),
  routes,
});

if (import.meta.hot) {
  handleHotUpdate(router);
}

const preset = definePreset(Aura, {
  semantic: {
    primary: {
      50: "{blue.50}",
      100: "{blue.100}",
      200: "{blue.200}",
      300: "{blue.300}",
      400: "{blue.400}",
      500: "{blue.500}",
      600: "{blue.600}",
      700: "{blue.700}",
      800: "{blue.800}",
      900: "{blue.900}",
      950: "{blue.950}",
    },
  },
});

createApp(App)
  .use(router)
  .use(PrimeVue, {
    theme: {
      preset,
    },
  })
  .use(oraAdminClient("http://127.0.0.1:50051"))
  .use(ConfirmationService)
  .mount("#app");
