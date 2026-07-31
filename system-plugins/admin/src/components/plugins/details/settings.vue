<script lang="ts" setup>
import { useToast } from "@/composables/useToast";
import { PluginStore, usePluginsStore } from "@/stores/plugins";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import { ref, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";

const route = useRoute(),
    store = usePluginsStore(),
    toast = useToast();

const props = defineProps<{ plugin: PluginStore }>();

// Settings state
const settingsSchema = ref<Record<string, any> | null>(null),
    settingsFormValues = ref<Record<string, any>>({}),
    settingsOriginalValues = ref<Record<string, any>>({}),
    settingsLoading = ref(false),
    settingsSaving = ref(false),
    settingsError = ref<string | null>(null);

onMounted(() => {
    loadSettings();
});

async function loadSettings() {
    await withAsyncHandlingVoid(
        settingsLoading,
        settingsError,
        async () => {
            const { settings, schema: fetchedSchema } =
                await store.fetchPluginSettings(route.params.name as string);
            if (fetchedSchema) {
                settingsSchema.value = fetchedSchema;
                settingsFormValues.value = settings ? { ...settings } : {};
                settingsOriginalValues.value = JSON.parse(
                    JSON.stringify(settings || {}),
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
            route.params.name as string,
            settingsFormValues.value,
        );
        settingsOriginalValues.value = JSON.parse(
            JSON.stringify(settingsFormValues.value),
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
    <form
        v-else-if="settingsSchema"
        @submit.prevent="saveSettings"
        class="space-y-6"
    >
        <div class="space-y-4">
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
    <div v-else class="text-gray-400 italic">
        This plugin does not have configurable settings.
    </div>
</template>
