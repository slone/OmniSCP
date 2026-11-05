import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { useLogStore } from "./stores/logs";

const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
// Before mounting, so the history comes ahead of this session's first entries.
useLogStore(pinia)
  .load()
  .finally(() => app.mount("#app"));
