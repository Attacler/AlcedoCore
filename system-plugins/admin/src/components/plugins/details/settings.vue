<script lang="ts" setup>
import { useToast } from "@/composables/useToast";
import { PluginStore, usePluginsStore } from "@/stores/plugins";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import { ref, onMounted } from "vue";

const store = usePluginsStore(),
    toast = useToast();

const props = defineProps<{ plugin: PluginStore; appVersionId: number }>();

// Settings state
const settingsSchema = ref<Record<string, any> | null>(null),
    settingsFormValues = ref<Record<string, any>>({}),
    settingsOriginalValues = ref<Record<string, any>>({}),
    settingsLoading = ref(false),
    settingsSaving = ref(false),
    settingsError = ref<string | null>(null);

const DEFAULT_LOG_IGNORE_PATHS = [".css", ".js"];
const logIgnorePaths = ref(DEFAULT_LOG_IGNORE_PATHS.join(", "));
const DEFAULT_LOG_RETENTION_DAYS = 30;
const logRetentionDays = ref<number>(DEFAULT_LOG_RETENTION_DAYS);
const systemSettingsOriginal = ref<Record<string, unknown>>({});

function toIgnoreString(list: unknown): string {
    return Array.isArray(list) && list.every((x) => typeof x === "string")
        ? (list as string[]).join(", ")
        : DEFAULT_LOG_IGNORE_PATHS.join(", ");
}

function parseIgnoreList(value: string): string[] {
    return value
        .split(",")
        .map((fragment) => fragment.trim())
        .filter(Boolean);
}

function toRetentionDays(value: unknown): number {
    return typeof value === "number" && Number.isFinite(value)
        ? Math.trunc(value)
        : DEFAULT_LOG_RETENTION_DAYS;
}

onMounted(() => {
    loadSettings();
});

async function loadSettings() {
    await withAsyncHandlingVoid(
        settingsLoading,
        settingsError,
        async () => {
            const {
                settings,
                system_settings: systemSettings,
                schema: fetchedSchema,
            } = await store.fetchPluginSettings(
                props.plugin.name,
                props.appVersionId,
            );
            systemSettingsOriginal.value = { ...(systemSettings ?? {}) };
            logIgnorePaths.value = toIgnoreString(
                systemSettings?.log_ignore_paths,
            );
            logRetentionDays.value = toRetentionDays(
                systemSettings?.log_retention_days,
            );
            if (fetchedSchema) {
                settingsSchema.value = fetchedSchema;

                const defaults: Record<string, any> = {};
                for (const [key, property] of Object.entries(
                    fetchedSchema.properties ?? {},
                )) {
                    if (
                        property &&
                        typeof property === "object" &&
                        "default" in property
                    ) {
                        defaults[key] = property.default;
                    }
                }
                settingsFormValues.value = { ...defaults, ...(settings ?? {}) };
                settingsOriginalValues.value = JSON.parse(
                    JSON.stringify(settingsFormValues.value),
                );
            } else {
                settingsSchema.value = null;
            }
        },
        "Failed to load settings",
    );
}

async function saveSettings() {
    settingsSaving.value = true;
    settingsError.value = null;
    try {
        await store.savePluginSettings(
            props.plugin.name,
            props.appVersionId,
            settingsFormValues.value,
        );
        await store.savePluginSystemSettings(
            props.plugin.name,
            props.appVersionId,
            {
                log_ignore_paths: parseIgnoreList(logIgnorePaths.value),
                log_retention_days: logRetentionDays.value,
            },
        );
        const { settings, system_settings: systemSettings } =
            await store.fetchPluginSettings(
                props.plugin.name,
                props.appVersionId,
            );
        settingsOriginalValues.value = JSON.parse(
            JSON.stringify(settings ?? {}),
        );
        systemSettingsOriginal.value = { ...(systemSettings ?? {}) };
        logIgnorePaths.value = toIgnoreString(systemSettings?.log_ignore_paths);
        logRetentionDays.value = toRetentionDays(
            systemSettings?.log_retention_days,
        );
        toast.show("Settings saved successfully", "success");
    } catch (e) {
        settingsError.value =
            e instanceof Error ? e.message : "Failed to save settings";
        toast.show(
            `Failed to save settings: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        settingsSaving.value = false;
    }
}

function resetSettings() {
    settingsFormValues.value = JSON.parse(
        JSON.stringify(settingsOriginalValues.value),
    );
    logIgnorePaths.value = toIgnoreString(
        systemSettingsOriginal.value.log_ignore_paths,
    );
    logRetentionDays.value = toRetentionDays(
        systemSettingsOriginal.value.log_retention_days,
    );
}
</script>

<template>
    <div v-if="settingsLoading" class="text-gray-500">Loading settings...</div>
    <div
        v-else-if="settingsError"
        class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
    >
        {{ settingsError }}
    </div>
    <form v-else @submit.prevent="saveSettings" class="space-y-6">
        <h3
            v-if="settingsSchema"
            class="text-sm font-semibold text-gray-700 uppercase tracking-wide"
        >
            Plugin settings
        </h3>
        <div class="space-y-4" v-if="settingsSchema">
            <div
                v-for="(property, fieldName) in settingsSchema.properties"
                :key="fieldName"
                class="flex flex-col gap-1"
            >
                <label class="text-sm font-medium text-gray-700">
                    {{ property.title || fieldName }}
                    <span
                        v-if="property.description"
                        class="block text-xs font-normal text-gray-500"
                        >{{ property.description }}</span
                    >
                </label>
                <InputText
                    v-if="property.type === 'string'"
                    v-model="settingsFormValues[String(fieldName)]"
                    type="text"
                    class="w-full"
                    fluid
                />
                <InputNumber
                    v-else-if="
                        property.type === 'integer' ||
                        property.type === 'number'
                    "
                    v-model.number="settingsFormValues[String(fieldName)]"
                    class="w-full"
                    fluid
                />
                <Checkbox
                    v-else-if="property.type === 'boolean'"
                    :binary="true"
                    v-model="settingsFormValues[String(fieldName)]"
                />
                <span v-else class="text-sm text-gray-500"
                    >Unsupported field type: {{ property.type }}</span
                >
            </div>
        </div>

        <h3
            class="text-sm font-semibold text-gray-700 uppercase tracking-wide pt-4 border-t border-gray-200"
        >
            System settings
        </h3>

        <div class="flex flex-col gap-1">
            <label class="text-sm font-medium text-gray-700">
                Ignored log paths
                <span class="block text-xs font-normal text-gray-500">
                    Requests whose URL contains any of these text fragments are
                    not written to the request logs. Separate multiple fragments
                    with commas. Case is ignored. Leave empty to record every
                    request.
                </span>
            </label>
            <InputText
                v-model="logIgnorePaths"
                type="text"
                class="w-full"
                fluid
            />
        </div>

        <div class="flex flex-col gap-1">
            <label class="text-sm font-medium text-gray-700">
                Log retention (days)
                <span class="block text-xs font-normal text-gray-500">
                    Requests older than this many days are deleted from the
                    request logs, once per hour. Set 0 to keep logs forever.
                </span>
            </label>
            <InputNumber
                v-model="logRetentionDays"
                :min="0"
                :max="3650"
                class="w-full"
                fluid
            />
        </div>

        <div class="flex gap-3 pt-4 border-t border-gray-200">
            <Button
                label="Save Settings"
                severity="primary"
                type="submit"
                :disabled="settingsSaving"
            />
            <Button
                label="Reset"
                severity="secondary"
                outlined
                :disabled="settingsSaving"
                @click="resetSettings"
            />
        </div>
    </form>
</template>
