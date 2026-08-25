<script setup lang="ts">
import {
    onMounted,
    onUnmounted,
    ref,
    shallowRef,
    computed,
    type Component,
} from "vue";
import { useRoute } from "vue-router";
import { useDevServerStore } from "@/stores/devServerStore";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";

const route = useRoute(),
    devServerStore = useDevServerStore(),
    extensionRegistry = useExtensionRegistryStore();

const pluginName = computed(() => (route.params.plugin as string) || ""),
    pagePath = computed(() => {
        const match = route.params.pathMatch;
        if (!match) return "/";
        return Array.isArray(match)
            ? "/" + match.join("/")
            : "/" + (match as string);
    });

const error = ref<string | null>(null),
    currentComponent = shallowRef(null as null | Component);

// Loading and state tracking
const loading = ref(false),
    notFound = ref(false),
    pageLabel = ref("");

const errorTitle = computed(() => {
    if (error.value?.includes("Failed to load")) return "Failed to load page";
    if (error.value?.includes("not found")) return "Page not found";
    return "Something went wrong";
});

const fullpage = ref(false);

const errorMessage = computed(() => {
    if (error.value)
        return error.value.replace(/^Failed to load plugin component: /, "");
    return "";
});

function fetchPageInfo() {
    try {
        const pages = extensionRegistry.getRoutesForPlugin(pluginName.value);
        const page = pages.find((p) => p.path === pagePath.value);

        if (!page) {
            // Check if this is a sub-page of a known manifest page
            const parentPage = pages.find((p) =>
                pagePath.value.startsWith(p.path + "/"),
            );
            if (parentPage) {
                pageLabel.value = parentPage.label || parentPage.path;
                currentComponent.value = parentPage.component;
                return true;
            }
            // Truly unknown page — show 404 but allow loadAssets to attempt anyway
            pageLabel.value = pagePath.value;
            return true;
        }
        pageLabel.value = page.label || page.path;
        currentComponent.value = page.component;
        loading.value = false;
        return true;
    } catch (e) {
        console.warn("Failed to fetch page info:", e);
        return false;
    }
}

// Load on mount and connect SSE
onMounted(async () => {
    console.log(
        "[PluginPage] mounted, route params:",
        route.params,
        "pathMatch:",
        route.params.pathMatch,
    );

    fetchPageInfo();

    devServerStore.connect();
});

onUnmounted(() => {
    devServerStore.disconnect();
});
</script>

<template>
    <div class="plugin-page" :class="{ fullpage: fullpage }">
        <!-- Loading spinner -->
        <div v-if="loading" class="flex items-center justify-center p-8">
            <div
                class="animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900"
            ></div>
            <span class="ml-3 text-gray-600">Loading page...</span>
        </div>

        <!-- Component -->
        <component
            :is="devServerStore.pageComponents[pagePath] || currentComponent"
            v-if="!loading && !error && !notFound"
            :class="fullpage ? 'fullpage-component' : ''"
        />
        <!-- Error boundary -->
        <div
            v-if="error && !loading"
            class="bg-red-50 border border-red-200 rounded-lg p-4"
        >
            <h3 class="text-red-800 font-medium flex items-center">
                <span class="mr-2">⚠</span>
                {{ errorTitle }}
            </h3>
            <p class="text-red-600 mt-1 text-sm">{{ errorMessage }}</p>
        </div>

        <!-- 404 page -->
        <div v-if="notFound && !loading" class="text-center py-12">
            <div class="text-6xl mb-4">🔍</div>
            <h3 class="text-xl font-medium text-gray-900 mb-2">
                Page not found
            </h3>
            <p class="text-gray-500 mb-4">
                The page "{{ pagePath }}" does not exist in this plugin.
            </p>
            <router-link
                :to="`/plugins/${pluginName}`"
                class="text-blue-600 hover:text-blue-800 underline"
            >
                Back to plugin
            </router-link>
        </div>
    </div>
</template>

<style scoped>
.plugin-page.fullpage {
    padding: 0;
    margin: 0;
    height: 100%;
}
</style>
