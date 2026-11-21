import { describe, it, expect, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createRouter, createMemoryHistory } from "vue-router";
import { createPinia } from "pinia";
import PluginList from "./PluginList.vue";
import { usePluginsStore } from "@/stores/plugins";

vi.mock("@/composables/useAlcedoClient", () => ({
    useAlcedoClient: () => ({
        client: {},
        isConnected: { value: true },
    }),
}));

vi.mock("@/stores/plugins", () => ({
    usePluginsStore: vi.fn().mockReturnValue({
        plugins: [
            {
                name: "test-plugin",
                version: "1.0.0",
                status: "enabled",
                plugin_type: "user",
                installations: [],
            },
        ],
        loading: false,
        error: null,
        fetchPlugins: vi.fn(),
        setInstallEnabled: vi.fn(),
        totalPlugins: 1,
        enabledPlugins: [],
        disabledPlugins: [],
    }),
}));

vi.mock("../composables/useToast", () => ({
    useToast: () => ({
        show: vi.fn(),
    }),
}));

const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: "/", component: { template: "<div />" } }],
});

function mountList() {
    return mount(PluginList, {
        global: {
            plugins: [router, createPinia()],
        },
    });
}

describe("PluginList.vue", () => {
    it("renders the plugins page and loads plugins via the data source", async () => {
        const wrapper = mountList();
        await flushPromises();
        expect(wrapper.text()).toContain("Plugins");
        expect(usePluginsStore().fetchPlugins).toHaveBeenCalled();
    });

    it("renders the table container", async () => {
        const wrapper = mountList();
        await flushPromises();
        expect(wrapper.find(".bg-white").exists()).toBe(true);
    });

    it("renders with an empty plugin list", async () => {
        const store = usePluginsStore();
        store.plugins = [];
        const wrapper = mountList();
        await flushPromises();
        expect(wrapper.text()).toContain("Plugins");
        store.plugins = [
            {
                name: "test-plugin",
                version: "1.0.0",
                status: "enabled",
                plugin_type: "user",
                installations: [],
            },
        ];
    });
});
