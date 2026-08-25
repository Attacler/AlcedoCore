<script lang="ts" setup>
import { PluginStore, usePluginsStore } from "@/stores/plugins";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import { ref } from "vue";
import { onMounted } from "vue";
import { useRoute } from "vue-router";

const props = defineProps<{ plugin: PluginStore }>();

const route = useRoute(),
    store = usePluginsStore();

const frontendLoading = ref(false),
    frontendError = ref<string | null>(null),
    frontendManifest = ref<any | null>(null);

async function loadFrontendManifest() {
    await withAsyncHandlingVoid(
        frontendLoading,
        frontendError,
        async () => {
            const module = await store.fetchPluginAssets(
                route.params.name as string,
            );
            frontendManifest.value = module.default || null;
        },
        "Failed to load frontend assets",
    );
}

onMounted(() => {
    loadFrontendManifest();
});
</script>

<template>
    <div v-if="frontendLoading" class="text-gray-500">
        Loading frontend assets...
    </div>
    <div
        v-else-if="frontendError"
        class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
    >
        {{ frontendError }}
    </div>
    <div v-else-if="frontendManifest">
        <!-- Pages section -->
        <div v-if="frontendManifest.pages?.length" class="mb-8">
            <h3
                class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-3 flex items-center gap-2"
            >
                <span>Pages</span>
                <span
                    class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full"
                    >{{ frontendManifest.pages.length }}</span
                >
            </h3>
            <div class="grid gap-4">
                <router-link
                    v-for="page in frontendManifest.pages"
                    :key="page.path"
                    :to="`/p/${route.params.name}${page.path}`"
                    class="block p-4 border border-gray-200 rounded-lg hover:bg-gray-50 hover:border-blue-300 transition-colors"
                >
                    <div class="flex items-center gap-3">
                        <span
                            v-if="page.icon"
                            class="text-2xl"
                            :class="page.icon"
                        ></span>
                        <div>
                            <h4 class="font-medium text-gray-900">
                                {{ page.label || page.title }}
                            </h4>
                            <p class="text-sm text-gray-500">
                                {{ page.path }}
                            </p>
                        </div>
                    </div>
                </router-link>
            </div>
        </div>
        <!-- Views section -->
        <div v-if="frontendManifest.views?.length" class="mb-8">
            <h3
                class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-3 flex items-center gap-2"
            >
                <span>Views</span>
                <span
                    class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full"
                    >{{ frontendManifest.views.length }}</span
                >
            </h3>
            <div class="grid gap-4">
                <div
                    v-for="view in frontendManifest.views"
                    :key="view.name"
                    class="block p-4 border border-gray-200 rounded-lg"
                >
                    <div class="flex items-center gap-3">
                        <span
                            v-if="view.icon"
                            class="text-2xl"
                            :class="view.icon"
                        ></span>
                        <div>
                            <h4 class="font-medium text-gray-900">
                                {{ view.label }}
                            </h4>
                            <p class="text-sm text-gray-500">
                                {{ view.name }}
                            </p>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        <!-- Inputs section -->
        <div v-if="frontendManifest.inputs?.length" class="mb-8">
            <h3
                class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-3 flex items-center gap-2"
            >
                <span>Inputs</span>
                <span
                    class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full"
                    >{{ frontendManifest.inputs.length }}</span
                >
            </h3>
            <div class="grid gap-4">
                <div
                    v-for="input in frontendManifest.inputs"
                    :key="input.name"
                    class="block p-4 border border-gray-200 rounded-lg"
                >
                    <div class="flex items-center gap-3">
                        <span
                            v-if="input.icon"
                            class="text-2xl"
                            :class="input.icon"
                        ></span>
                        <div>
                            <h4 class="font-medium text-gray-900">
                                {{ input.label }}
                            </h4>
                            <p class="text-sm text-gray-500">
                                {{ input.name }}
                            </p>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        <!-- Displays section -->
        <div v-if="frontendManifest.displays?.length" class="mb-8">
            <h3
                class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-3 flex items-center gap-2"
            >
                <span>Displays</span>
                <span
                    class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full"
                    >{{ frontendManifest.displays.length }}</span
                >
            </h3>
            <div class="grid gap-4">
                <div
                    v-for="display in frontendManifest.displays"
                    :key="display.name"
                    class="block p-4 border border-gray-200 rounded-lg"
                >
                    <div class="flex items-center gap-3">
                        <span
                            v-if="display.icon"
                            class="text-2xl"
                            :class="display.icon"
                        ></span>
                        <div>
                            <h4 class="font-medium text-gray-900">
                                {{ display.label }}
                            </h4>
                            <p class="text-sm text-gray-500">
                                {{ display.name }}
                            </p>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        <div
            v-if="
                !frontendManifest.pages?.length &&
                !frontendManifest.views?.length &&
                !frontendManifest.inputs?.length &&
                !frontendManifest.displays?.length
            "
            class="text-gray-400 italic"
        >
            No frontend assets registered for this plugin
        </div>
    </div>
    <div v-else class="text-gray-400 italic">
        No frontend assets available for this plugin
    </div>
</template>
