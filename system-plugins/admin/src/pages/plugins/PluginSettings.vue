<script setup lang="ts">
import { ref, reactive, onMounted, watch } from "vue";
import { useRoute } from "vue-router";
import { usePluginsStore } from "@/stores/plugins";
import { useToast } from "@/composables/useToast";
import DynamicFormField from "@/components/plugins/DynamicFormField.vue";
import type { JsonSchema } from "@/types/dynamic-form";
import { validateForm } from "@/types/dynamic-form";

const route = useRoute(),
    store = usePluginsStore(),
    toast = useToast();

const pluginName = route.params.name as string,
    loading = ref(false),
    saving = ref(false),
    originalSettings = ref<Record<string, unknown>>({});

const schema = ref<JsonSchema | null>(null),
    formErrors = ref<Record<string, string | undefined>>({}),
    formValues = reactive<Record<string, unknown>>({});

// Debounce helper
function debounce<T extends (...args: unknown[]) => void>(
    fn: T,
    ms: number,
): (...args: Parameters<T>) => void {
    let timeoutId: ReturnType<typeof setTimeout>;
    return (...args: Parameters<T>) => {
        clearTimeout(timeoutId);
        timeoutId = setTimeout(() => fn(...args), ms);
    };
}

onMounted(async () => {
    loading.value = true;
    try {
        const { settings, schema: fetchedSchema } =
            await store.fetchPluginSettings(pluginName);
        if (fetchedSchema) {
            schema.value = fetchedSchema as unknown as JsonSchema;
        }
        if (settings) {
            Object.assign(formValues, settings);
            originalSettings.value = JSON.parse(JSON.stringify(settings));
        }
    } catch (e) {
        toast.show(
            `Failed to load settings: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        loading.value = false;
    }
});

// Real-time validation with debounce
const debouncedValidate = debounce(() => {
    if (schema.value) {
        formErrors.value = validateForm(formValues, schema.value);
    }
}, 300);

watch(
    formValues,
    () => {
        debouncedValidate();
    },
    { deep: true },
);

async function saveSettings() {
    saving.value = true;
    try {
        await store.savePluginSettings(
            pluginName,
            formValues as Record<string, unknown>,
        );
        toast.show("Settings saved successfully", "success");
        originalSettings.value = JSON.parse(JSON.stringify(formValues));
    } catch (e) {
        toast.show(
            `Failed to save: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        saving.value = false;
    }
}

function resetForm() {
    Object.keys(formValues).forEach((key) => {
        formValues[key] = originalSettings.value[key];
    });
}
</script>

<template>
    <div class="p-6 max-w-2xl">
        <router-link
            :to="`/plugins/${pluginName}/`"
            class="inline-block mb-4 text-blue-500 text-sm hover:underline"
            >← Back to Detail</router-link
        >

        <h1 class="text-2xl font-bold mb-6">Settings: {{ pluginName }}</h1>

        <div v-if="loading" class="p-8 text-center text-gray-500">
            Loading settings...
        </div>

        <!-- No schema warning -->
        <div
            v-else-if="!schema"
            class="p-6 bg-yellow-50 text-yellow-700 rounded-lg"
        >
            This plugin does not have configurable settings.
        </div>

        <form
            v-else
            @submit.prevent="saveSettings"
            class="bg-white rounded-lg shadow-sm"
        >
            <div class="p-6 border-b border-gray-200">
                <DynamicFormField
                    v-for="(property, fieldName) in schema.properties"
                    :key="fieldName"
                    :field-name="fieldName"
                    :property="property"
                    v-model="formValues[fieldName]"
                    :error="formErrors[fieldName]"
                />
            </div>

            <div class="p-4 bg-gray-50 rounded-b-lg flex gap-3">
                <button
                    type="submit"
                    :disabled="
                        saving ||
                        Object.keys(formErrors).filter((k) => formErrors[k])
                            .length > 0
                    "
                    class="px-5 py-2.5 bg-blue-500 text-white rounded-md text-sm font-medium hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                    Save Settings
                </button>
                <button
                    type="button"
                    @click="resetForm"
                    :disabled="saving"
                    class="px-5 py-2.5 bg-white border border-gray-300 text-gray-700 rounded-md text-sm font-medium hover:bg-gray-50 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                    Reset
                </button>
            </div>
        </form>
    </div>
</template>
