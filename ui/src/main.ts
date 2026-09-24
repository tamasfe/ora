import { createApp } from "vue";
import "./style.css";
import App from "./App.vue";
import { createRouter, createWebHistory } from "vue-router";
import { handleHotUpdate, routes } from "vue-router/auto-routes";
import PrimeVue from "primevue/config";
import Aura from "@primeuix/themes/aura";
import { definePreset } from "@primeuix/themes";
import ConfirmationService from "primevue/confirmationservice";
import ToastService from "primevue/toastservice";
import Tooltip from "primevue/tooltip";

import "primeicons/primeicons.css";
import { oraAdminClient } from "./grpc";

/**
 * The path the UI is served under, set by the server via the <base> element.
 */
function basePath(): string {
  const href = document.querySelector("base")?.href;
  return href ? new URL(href, location.href).pathname : "/";
}

/**
 * The URL of the Ora API, the server can set it via a meta tag,
 * by default the API is expected to be served on the same origin as the UI.
 */
function apiUrl(): string {
  const configured = document
    .querySelector<HTMLMetaElement>('meta[name="ora-api-url"]')
    ?.content.trim();

  return configured || import.meta.env.VITE_ORA_API_URL || location.origin;
}

/**
 * Whether credentials (e.g. cookies) are sent to an API on a different origin,
 * the server can enable it via a meta tag.
 */
function apiCredentials(): RequestCredentials {
  const configured =
    document
      .querySelector<HTMLMetaElement>('meta[name="ora-api-credentials"]')
      ?.content.trim() || import.meta.env.VITE_ORA_API_CREDENTIALS;

  return configured === "include" ? "include" : "same-origin";
}

const router = createRouter({
  history: createWebHistory(basePath()),
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
  .use(oraAdminClient(apiUrl(), { credentials: apiCredentials() }))
  .use(ConfirmationService)
  .use(ToastService)
  .directive("tooltip", Tooltip)
  .mount("#app");
