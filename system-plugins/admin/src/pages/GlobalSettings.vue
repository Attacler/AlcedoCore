<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useSettingsStore } from "@/stores/settingsStore";
import { useToast } from "@/composables/useToast";

const store = useSettingsStore(),
    toast = useToast();

const platformName = ref(""),
    saving = ref(false);

onMounted(async () => {
    if (store.platformSettings.platform_name === "Loading") {
        await store.fetchPlatformSettings();
    }
    platformName.value = store.platformSettings.platform_name;
});

async function save() {
    const name = platformName.value.trim();
    if (!name) {
        toast.show("Platform name is required", "warning");
        return;
    }
    saving.value = true;
    try {
        await store.updatePlatformName(name);
        platformName.value = store.platformSettings.platform_name;
        toast.show("Platform name saved", "success");
    } catch (e) {
        toast.show(
            `Failed to save: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        saving.value = false;
    }
}
</script>

<template>
    <div class="space-y-6">
        <div class="flex items-center justify-between mb-6">
            <h1 class="text-2xl font-semibold text-gray-900">
                System Settings
            </h1>
        </div>

        <div
            class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden max-w-2xl"
        >
            <div class="px-4 py-3">
                <div class="text-sm font-medium text-gray-900">
                    Platform Name
                </div>
                <div class="text-xs text-gray-500 mt-0.5">
                    Displayed in the sidebar header, browser title and login
                    screen.
                </div>
            </div>
            <div
                class="flex items-center gap-3 px-4 py-3 border-t border-gray-100"
            >
                <InputText
                    v-model="platformName"
                    placeholder="AlcedoCore"
                    class="flex-1"
                    fluid
                    @keyup.enter="save"
                />
                <Button
                    :label="saving ? 'Saving...' : 'Save'"
                    :disabled="
                        saving ||
                        !platformName.trim() ||
                        platformName.trim() ===
                            store.platformSettings.platform_name
                    "
                    @click="save"
                />
            </div>
        </div>
    </div>
</template>
