<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRouter, onBeforeRouteLeave } from "vue-router";
import {
    useSettingsStore,
    CATEGORIES,
    SETTING_META,
} from "@/stores/settingsStore";
import { useToast } from "@/composables/useToast";
import { useDevMode } from "@/composables/useDevMode";
import Select from "primevue/select";
import ToggleSwitch from "primevue/toggleswitch";
import MenuBuilder from "./MenuBuilder.vue";
import SettingsSession from "./SettingsSession.vue";
import DeveloperKeysDrawer from "@/components/developerSettings/keysDrawer.vue";
import FileUpload from "@/components/inputs/FileUpload.vue";
import { MediaFile } from "alcedocore-sdk-node";
import EnableDevelopmentMode from "./development/enableDevelopmentMode.vue";

const props = defineProps<{ category: string }>();
const router = useRouter(),
    store = useSettingsStore(),
    toast = useToast(),
    { devMode: devModeEnabled, toggle: toggleDevMode } = useDevMode();

const savingKey = ref<string | null>(null),
    showResetConfirm = ref(false),
    showUnsavedDialog = ref(false),
    pendingNavigation = ref<((value?: any) => void) | null>(null),
    uploadingKey = ref<string | null>(null);

const categoryDef = computed(() =>
    CATEGORIES.find((c) => c.id === props.category),
);

const settingsKeys = computed(() => store.getCategorySettings(props.category));

watch(categoryDef, (def) => {
    if (!def && !store.loading) {
        router.replace("/settings");
    }
});

async function handleSave(key: string) {
    const error = store.validateValue(key);
    if (error) {
        toast.show(error, "warning");
        return;
    }
    savingKey.value = key;
    try {
        await store.saveSetting(key);
        toast.show(`"${SETTING_META[key]?.label || key}" saved`, "success");
    } catch (e) {
        toast.show(
            `Failed to save: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        savingKey.value = null;
    }
}

function handleCancel() {
    store.cancelAll();
    toast.show("Changes reverted", "info");
}

async function handleReset() {
    showResetConfirm.value = false;
    try {
        await store.resetToDefaults();
        toast.show("Settings restored to defaults", "success");
    } catch (e) {
        toast.show(
            `Failed to reset: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}

onBeforeRouteLeave((_to, _from, next) => {
    if (store.isDirty) {
        showUnsavedDialog.value = true;
        pendingNavigation.value = next;
    } else {
        next();
    }
});

function confirmLeave() {
    showUnsavedDialog.value = false;
    if (pendingNavigation.value) {
        pendingNavigation.value();
        pendingNavigation.value = null;
    }
}

function cancelLeave() {
    showUnsavedDialog.value = false;
    pendingNavigation.value = null;
}

onMounted(() => {
    store.fetchSettings();
});

function fileUploaded(key: string, uploadResponse: MediaFile) {
    store.updateLocalValue(key, uploadResponse.download_url);
    toast.show("Image uploaded", "success");
    uploadingKey.value = null;
}
</script>

<template>
    <MenuBuilder v-if="category === 'menu'" />
    <SettingsSession v-else-if="category === 'session'" />
    <div v-else class="space-y-6">
        <div class="flex items-center justify-between mb-6">
            <div class="flex items-center gap-3">
                <router-link
                    to="/settings"
                    class="material-symbols-outlined text-gray-400 hover:text-gray-600 transition-colors"
                >
                    arrow_back
                </router-link>
                <div class="flex items-center gap-2">
                    <span
                        v-if="categoryDef"
                        class="material-symbols-outlined text-gray-500 text-xl"
                        >{{ categoryDef.icon }}</span
                    >
                    <div>
                        <h1 class="text-2xl font-semibold text-gray-900">
                            {{ categoryDef?.label || "Settings" }}
                        </h1>
                        <p v-if="categoryDef" class="text-sm text-gray-500">
                            {{ categoryDef.description }}
                        </p>
                    </div>
                </div>
            </div>
        </div>

        <div v-if="store.loading">
            <div class="space-y-4">
                <div
                    class="bg-white rounded-lg shadow-sm border border-gray-200 p-4"
                >
                    <div class="flex items-center gap-3 mb-4">
                        <div
                            class="w-5 h-5 bg-gray-200 rounded animate-pulse"
                        ></div>
                        <div
                            class="h-5 bg-gray-200 rounded animate-pulse w-32"
                        ></div>
                    </div>
                    <div
                        class="h-4 bg-gray-200 rounded animate-pulse w-48 mb-4"
                    ></div>
                    <div
                        v-for="m in 2"
                        :key="m"
                        class="flex items-center gap-3 mb-3"
                    >
                        <div class="flex-1">
                            <div
                                class="h-3 bg-gray-200 rounded animate-pulse w-24 mb-2"
                            ></div>
                            <div
                                class="h-3 bg-gray-200 rounded animate-pulse w-40"
                            ></div>
                        </div>
                        <div
                            class="h-8 bg-gray-200 rounded animate-pulse w-48"
                        ></div>
                        <div
                            class="h-8 bg-gray-200 rounded animate-pulse w-16"
                        ></div>
                    </div>
                </div>
            </div>
        </div>

        <div
            v-else-if="store.error"
            class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4"
        >
            <div class="flex items-center gap-2 mb-2">
                <span class="material-symbols-outlined text-lg">error</span>
                <span class="font-medium">Failed to load settings</span>
            </div>
            <p class="text-sm mb-3">{{ store.error }}</p>
            <Button
                label="Retry"
                severity="warn"
                @click="store.fetchSettings()"
            />
        </div>

        <div
            v-else-if="settingsKeys.length === 0 && category !== 'developer'"
            class="bg-white rounded-lg shadow-sm border border-gray-200 p-6 text-center"
        >
            <span
                class="material-symbols-outlined text-4xl text-gray-300 mb-3"
                >{{ categoryDef?.icon || "settings" }}</span
            >
            <h3 class="text-lg font-medium text-gray-900 mb-2">No settings</h3>
            <p class="text-gray-500 text-sm">
                No settings configured in this category.
            </p>
        </div>

        <!-- Developer -->
        <div
            v-else-if="category === 'developer'"
            class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden"
        >
            <div class="divide-y divide-gray-100 grid gap-2">
                <div class="flex items-center justify-between px-4 py-3">
                    <div class="flex-1 mr-4 min-w-0">
                        <div class="text-sm font-medium text-gray-900">
                            Show Development Mode
                        </div>
                        <div class="text-xs text-gray-500 mt-0.5">
                            Display code icons next to field names showing API
                            name, type, and display type
                        </div>
                    </div>
                    <div class="flex items-center gap-3 shrink-0">
                        <ToggleSwitch
                            :modelValue="devModeEnabled"
                            @update:modelValue="toggleDevMode"
                        />
                    </div>
                </div>
                <div class="flex items-center justify-between px-4 py-3">
                    <div class="flex-1 mr-4 min-w-0">
                        <div class="text-sm font-medium text-gray-900">
                            API keys
                        </div>
                        <div class="text-xs text-gray-500 mt-0.5">
                            View, create or delete keys. The keys are can be
                            used to easily interact with the API.
                        </div>
                    </div>
                    <DeveloperKeysDrawer />
                </div>
                <routerLink
                    to="/apidocs"
                    class="flex items-center justify-between px-4 py-3"
                >
                    <div class="flex-1 mr-4 min-w-0">
                        <div class="text-sm font-medium text-gray-900">
                            API documentation
                        </div>
                        <div class="text-xs text-gray-500 mt-0.5">
                            View the core API documentation
                        </div>
                    </div>
                    <Button label="Open documentation" />
                </routerLink>
                <div
                    to="/apidocs"
                    class="flex items-center justify-between px-4 py-3"
                >
                    <div class="flex-1 mr-4 min-w-0">
                        <div class="text-sm font-medium text-gray-900">
                            Development mode
                        </div>
                        <div class="text-xs text-gray-500 mt-0.5">
                            Enable development mode to easily develop frontend
                            plugin assets.
                        </div>
                    </div>
                    <EnableDevelopmentMode />
                </div>
            </div>
        </div>

        <div
            v-else
            class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden"
        >
            <div class="divide-y divide-gray-100">
                <div
                    v-for="key in settingsKeys"
                    :key="key"
                    class="md:flex items-center justify-between px-4 py-3"
                >
                    <div class="flex-1 mr-4 min-w-0">
                        <div class="text-sm font-medium text-gray-900">
                            {{ SETTING_META[key]?.label || key }}
                        </div>
                        <div
                            v-if="SETTING_META[key]?.description"
                            class="text-xs text-gray-500 mt-0.5"
                        >
                            {{ SETTING_META[key].description }}
                        </div>
                    </div>
                    <div
                        class="flex items-center gap-3 shrink-0 max-w-[320px] w-full sm:max-w-70"
                    >
                        <div class="flex-1 min-w-0">
                            <Select
                                v-if="SETTING_META[key]?.type === 'select'"
                                :modelValue="store.getLocalValue(key)"
                                @update:modelValue="
                                    store.updateLocalValue(key, $event)
                                "
                                :options="
                                    store.dynamicOptions[key] ||
                                    SETTING_META[key]?.options
                                "
                                option-label="label"
                                option-value="value"
                                class="w-full"
                                :invalid="!!store.validateValue(key)"
                            />
                            <ToggleSwitch
                                v-else-if="
                                    SETTING_META[key]?.type === 'boolean'
                                "
                                :modelValue="store.getLocalValue(key)"
                                @update:modelValue="
                                    store.updateLocalValue(key, $event)
                                "
                            />
                            <div
                                v-else-if="
                                    SETTING_META[key]?.type === 'file-image'
                                "
                                class="flex items-center gap-3"
                            >
                                <img
                                    v-if="store.getLocalValue(key)"
                                    :src="store.getLocalValue(key)"
                                    class="w-10 h-10 object-contain rounded border border-gray-200"
                                />
                                <span
                                    v-else
                                    class="w-10 h-10 flex items-center justify-center rounded border border-gray-200 bg-gray-50 text-gray-400 text-xs"
                                    >No image</span
                                >
                                <FileUpload
                                    @file-uploaded="fileUploaded(key, $event)"
                                    :folderID="null"
                                >
                                    <template #default="{ openFileupload }">
                                        <Button
                                            icon="pi pi-upload"
                                            severity="secondary"
                                            outlined
                                            size="small"
                                            @click="openFileupload"
                                            :loading="uploadingKey === key"
                                        />
                                    </template>
                                </FileUpload>
                                <Button
                                    v-if="store.getLocalValue(key)"
                                    icon="pi pi-trash"
                                    severity="danger"
                                    text
                                    size="small"
                                    @click="store.updateLocalValue(key, '')"
                                />
                            </div>
                            <InputText
                                v-else-if="SETTING_META[key]?.type !== 'number'"
                                :value="store.getLocalValue(key)"
                                @input="
                                    store.updateLocalValue(
                                        key,
                                        ($event.target as HTMLInputElement)
                                            .value,
                                    )
                                "
                                :invalid="!!store.validateValue(key)"
                                class="w-full"
                                fluid
                            />
                            <InputNumber
                                v-else-if="SETTING_META[key]?.type === 'number'"
                                :value="store.getLocalValue(key)"
                                @input="
                                    store.updateLocalValue(key, $event.value)
                                "
                                :invalid="!!store.validateValue(key)"
                                class="w-full"
                                fluid
                            />
                            <span v-else class="text-sm text-gray-500"
                                >Unsupported type:
                                {{ SETTING_META[key]?.type }}</span
                            >
                            <div
                                v-if="store.validateValue(key)"
                                class="text-xs text-red-500 mt-1"
                            >
                                {{ store.validateValue(key) }}
                            </div>
                        </div>
                        <Button
                            :label="savingKey === key ? 'Saving...' : 'Save'"
                            :disabled="
                                savingKey === key ||
                                store.getLocalValue(key) ===
                                    store.settings[key] ||
                                !!store.validateValue(key)
                            "
                            severity="secondary"
                            outlined
                            @click="handleSave(key)"
                        />
                    </div>
                </div>
            </div>
        </div>

        <div
            v-if="!store.loading && !store.error && settingsKeys.length > 0"
            class="sticky bottom-0 bg-white border-t border-gray-200 px-6 py-3 flex gap-3 max-sm:flex-col"
        >
            <Button
                label="Cancel"
                severity="secondary"
                outlined
                :disabled="!store.isDirty"
                @click="handleCancel"
            />
            <Button
                label="Reset to Defaults"
                severity="danger"
                @click="showResetConfirm = true"
            />
        </div>

        <Dialog
            v-model:visible="showResetConfirm"
            header="Reset to Defaults"
            :modal="true"
            :style="{ width: '450px' }"
            :draggable="false"
        >
            <p class="text-gray-600 mb-4">
                Reset all settings to defaults? This cannot be undone.
            </p>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="showResetConfirm = false"
                />
                <Button label="Reset" severity="danger" @click="handleReset" />
            </template>
        </Dialog>

        <Dialog
            v-model:visible="showUnsavedDialog"
            header="Unsaved Changes"
            :modal="true"
            :style="{ width: '450px' }"
            :draggable="false"
        >
            <p class="text-gray-600 mb-4">
                You have unsaved changes. Leave without saving?
            </p>
            <template #footer>
                <Button
                    label="Stay"
                    severity="secondary"
                    outlined
                    @click="cancelLeave"
                />
                <Button
                    label="Discard"
                    severity="danger"
                    @click="confirmLeave"
                />
            </template>
        </Dialog>
    </div>
</template>
