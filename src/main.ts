import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { useLogStore } from "./stores/logs";
import { useSettingsStore } from "./stores/settings";

const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
// Before mounting, so the history comes ahead of this session's first entries
// and the window opens in the chosen theme.
Promise.allSettled([useLogStore(pinia).load(), useSettingsStore(pinia).load()]).finally(() =>
  app.mount("#app"),
);
