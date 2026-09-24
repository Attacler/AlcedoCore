import * as vue from "vue";
// Expose Vue globals for plugin page-compiler (page-compiler assets reference window.vue)
if (!(window as any).vue) {
    (window as any).vue = vue;
}
import { createPinia } from "pinia";
import PrimeVue from "primevue/config";
import ConfirmationService from "primevue/confirmationservice";
import Aura from "@primevue/themes/aura";
import App from "./App.vue";
import router from "./router";
import "primeicons/primeicons.css";
import "./style.css";
import FilterBuilder from "./components/FilterBuilder.vue";
import { Tooltip } from "primevue";
import { setAppHeaders, getAppHeaders } from "./utils/appHeaders";
import { useAppContextStore } from "./stores/appContext";
// Expose FilterBuilder globally for plugin pages to use
if (!(window as any).FilterBuilder) {
    (window as any).FilterBuilder = FilterBuilder;
}

// Intercept fetch globally — inject app context headers and redirect to login on 401
const origFetch = window.fetch.bind(window);
window.fetch = (input: RequestInfo | URL, init?: RequestInit) => {
    const baseHeaders =
        init?.headers ?? (input instanceof Request ? input.headers : undefined);
    const headers = new Headers(baseHeaders);
    const ctx = getAppHeaders();
    if (ctx.app && !headers.has("X-App")) headers.set("X-App", ctx.app);
    if (ctx.version && !headers.has("X-Version")) headers.set("X-Version", ctx.version);
    return origFetch(input, { ...init, headers }).then((res) => {
        if (res.status === 401 && !res.url.includes("/api/auth/login")) {
            window.location.hash = "#/login";
        }
        return res;
    });
};

const app = vue.createApp(App);
app.use(createPinia());
const appContextStore = useAppContextStore();
appContextStore.$subscribe((_mutation, state) => {
    setAppHeaders(state.appSlug, state.version);
});
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
app.use(ConfirmationService);

app.mount("#app");
