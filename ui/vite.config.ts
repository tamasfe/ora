import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import VueRouter from "vue-router/vite";

import Components from "unplugin-vue-components/vite";
import { PrimeVueResolver } from "@primevue/auto-import-resolver";

// https://vite.dev/config/
export default defineConfig({
  // Relative asset URLs, so that the UI can be served under any path prefix.
  // The actual prefix is set via the <base> element by the server.
  base: "./",
  plugins: [
    VueRouter({
      routesFolder: "src/views",
    }),
    tailwindcss(),
    vue(),
    Components({
      resolvers: [PrimeVueResolver()],
    }),
  ],
});
