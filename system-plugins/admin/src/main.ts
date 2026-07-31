import * as vue from "vue";
// Expose Vue globals for plugin page-compiler (page-compiler assets reference window.vue)
if (!(window as any).vue) {
    (window as any).vue = vue;
}
import { createPinia } from "pinia";
import PrimeVue from "primevue/config";
import Aura from "@primevue/themes/aura";
import App from "./App.vue";
import router from "./router";
import "primeicons/primeicons.css";
import "./style.css";
import FilterBuilder from "./components/FilterBuilder.vue";
import { Tooltip } from "primevue";
// Expose FilterBuilder globally for plugin pages to use
if (!(window as any).FilterBuilder) {
    (window as any).FilterBuilder = FilterBuilder;
}

// Intercept 401 responses globally — redirect to login on session expiry
const origFetch = window.fetch.bind(window);
window.fetch = (input: RequestInfo | URL, init?: RequestInit) => {
    return origFetch(input, init).then((res) => {
        if (res.status === 401 && !res.url.includes("/api/auth/login")) {
            window.location.hash = "#/login";
        }
        return res;
    });
};

const app = vue.createApp(App);
app.use(createPinia());
app.use(router);
app.use(PrimeVue, {
    theme: {
        preset: Aura,
        options: {
            darkModeSelector: false,
            cssLayer: {
                name: "primevue",
                order: "tailwind-base, primevue, tailwind-utilities",
            },
        },
    },
});
app.directive("tooltip", Tooltip);

app.mount("#app");
