import { defineStore } from "pinia";
import { ref } from "vue";

export const useAppContextStore = defineStore("appContext", () => {
    const appSlug = ref<string | null>(null);
    const version = ref<string | null>(null);

    function setContext(app: string | null, ver: string | null) {
        appSlug.value = app;
        version.value = ver;
    }

    function clear() {
        appSlug.value = null;
        version.value = null;
    }

    return { appSlug, version, setContext, clear };
});
