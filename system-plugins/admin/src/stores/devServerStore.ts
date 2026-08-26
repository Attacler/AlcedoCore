import { defineStore } from "pinia";
import { ref, computed, ShallowRef } from "vue";

export const useDevServerStore = defineStore("devServer", () => {
    const devServerUrl = ref("http://localhost:3003"),
        pluginManifest = ref({
            pluginSlug: "",
            version: "",
        }),
        pluginDetails = ref<any>({});

    const connected = ref(false),
        enabled = ref(false),
        connectionError = ref(false),
        reconnectAttempts = ref(0),
        eventSource = ref<EventSource | null>(null),
        maxReconnectAttempts = 2,
        baseDelay = 1000;

    const pageComponents = ref<{ [key: string]: ShallowRef<any> }>({}),
        displayComponents = ref<{ [key: string]: ShallowRef<any> }>({}),
        displaySettingsComponents = ref<{ [key: string]: ShallowRef<any> }>({}),
        inputComponents = ref<{ [key: string]: ShallowRef<any> }>({}),
        componentStyling = ref<{ [key: string]: string[] }>({});

    const isDevMode = computed(() => connected.value);

    function scheduleReconnect() {
        if (reconnectAttempts.value >= maxReconnectAttempts) {
            console.warn("[DevServer] Max reconnect attempts reached");
            connectionError.value = true;
            resetDevState();
            return;
        }

        const delay = baseDelay * Math.pow(2, reconnectAttempts.value);
        reconnectAttempts.value++;

        setTimeout(() => {
            connect();
        }, delay);
    }

    function connect() {
        enabled.value = true;

        if (eventSource.value) {
            eventSource.value.close();
        }

        const es = new EventSource(`${devServerUrl.value}/streaming`);
        eventSource.value = es;

        es.onopen = () => {
            localStorage.setItem("dev-state", "enabled");
            console.log("[DevServer] SSE connected");
            connected.value = true;
            reconnectAttempts.value = 0;
            loadComponentJS().then(() => loadComponentCSS());
        };

        es.addEventListener("reloadJS", () => {
            loadComponentJS();
        });
        es.addEventListener("reloadCSS", () => {
            loadComponentCSS();
        });

        es.onerror = () => {
            console.log("[DevServer] SSE disconnected");
            connected.value = false;
            es.close();
            scheduleReconnect();
        };
    }

    function disconnect() {
        if (eventSource.value) {
            eventSource.value.close();
            eventSource.value = null;
        }
        connected.value = false;

        resetDevState();
    }

    function resetDevState() {
        const key = "dev:" + pluginManifest.value.pluginSlug;
        const currentIDs = componentStyling.value[key];

        if (currentIDs) {
            for (const styleID of currentIDs) {
                for (const elm of document.querySelectorAll(
                    "style[cid='" + styleID + "']",
                )) {
                    elm.remove();
                }
            }
        }
        pageComponents.value = {};
        displayComponents.value = {};
        displaySettingsComponents.value = {};
        inputComponents.value = {};
        pluginDetails.value = {};
        localStorage.removeItem("dev-state");
    }

    async function loadComponentJS() {
        try {
            const content = await fetch(devServerUrl.value + "/dev/js").then(
                (e) => e.json(),
            );

            const { default: module } = await doimport(content.code);
            // console.log({ module });

            for (const page of module.pages) {
                pageComponents.value[page.path] = page.component;
            }
            for (const display of module.displays) {
                displayComponents.value[display.name] = display.component;

                if (display.settingsComponent) {
                    displaySettingsComponents.value[display.name] =
                        display.settingsComponent;
                }
            }
            for (const input of module.inputs) {
                inputComponents.value[input.name] = input.component;
            }
            // console.log(pageComponents.value);

            pluginManifest.value = {
                pluginSlug: module.pluginSlug,
                version: module.manifestVersion,
            };
            pluginDetails.value = module;
        } catch (error) {
            console.error(`Error loading component:`, error);
        }
    }
    async function loadComponentCSS() {
        const content = await fetch(devServerUrl.value + "/dev/css").then((e) =>
            e.text(),
        );

        const id = (Math.random() + "").replace(".", "");
        const key = "dev:" + pluginManifest.value.pluginSlug;
        const currentIDs = componentStyling.value[key];

        if (currentIDs) {
            for (const styleID of currentIDs) {
                for (const elm of document.querySelectorAll(
                    "style[cid='" + styleID + "']",
                )) {
                    elm.remove();
                }
            }
        }

        if (content) {
            const style = document.createElement("style");
            style.innerHTML = content;
            style.setAttribute("cid", id);
            document.head.appendChild(style);

            if (!componentStyling.value[key]) componentStyling.value[key] = [];

            componentStyling.value[key].push(id);
        }
    }

    function doimport(str: string) {
        if ((globalThis as any).URL.createObjectURL) {
            const blob = new Blob([str], { type: "text/javascript" });
            const url = URL.createObjectURL(blob);
            const module = import(/* @vite-ignore */ url);
            URL.revokeObjectURL(url); // GC objectURLs
            return module;
        }

        const url = "data:text/javascript;base64," + btoa(str);
        return import(/* @vite-ignore */ url);
    }

    if (localStorage.getItem("dev-state") == "enabled") {
        connect();
    }
    return {
        devServerUrl,
        connected,
        connectionError,
        isDevMode,
        connect,
        disconnect,
        enabled,
        pageComponents,
        displayComponents,
        displaySettingsComponents,
        inputComponents,
        pluginManifest,
        pluginDetails,
    };
});
