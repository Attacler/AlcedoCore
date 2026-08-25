<script setup lang="ts">
import { computed } from "vue";
import type { FieldDefinition } from "@/stores/collections";
import FieldNameLabel from "@/components/FieldNameLabel.vue";
import Select from "primevue/select";
import Checkbox from "primevue/checkbox";
import Button from "primevue/button";
import TemplateInput from "@/components/TemplateInput.vue";

const props = defineProps<{
    fields: FieldDefinition[];
    settings: Record<string, any>;
    onChange: (key: string, value: any) => void;
}>();

const sortOptions = computed(() => [
    { label: "None", value: "" },
    ...props.fields.map((f) => ({
        label: f.display_name || f.name,
        value: f.name,
    })),
]);

const titleTemplate = computed(() => props.settings?.titleTemplate || "");

const displayFields = computed(
    () => (props.settings?.displayFields as string[]) || [],
);

function toggleSortOrder() {
    const current =
        (props.settings?.sortOrder || "asc") === "asc" ? "desc" : "asc";
    props.onChange("sortOrder", current);
}

function isDisplayField(name: string): boolean {
    return displayFields.value.includes(name);
}

function toggleDisplay(name: string) {
    const current = displayFields.value;
    const next = current.includes(name)
        ? current.filter((n) => n !== name)
        : [...current, name];
    props.onChange("displayFields", next);
}
</script>

<template>
    <div class="space-y-4">
        <div class="space-y-3">
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Sort By</label
                >
                <div class="flex gap-2">
                    <Select
                        :model-value="settings?.sortField || ''"
                        @update:model-value="(v) => onChange('sortField', v)"
                        :options="sortOptions"
                        option-label="label"
                        option-value="value"
                        placeholder="None"
                        class="w-full"
                    />
                    <Button
                        v-if="settings?.sortField"
                        :icon="
                            (settings?.sortOrder || 'asc') === 'asc'
                                ? 'pi pi-sort-amount-up-alt'
                                : 'pi pi-sort-amount-down'
                        "
                        text
                        severity="secondary"
                        rounded
                        @click="toggleSortOrder"
                    />
                </div>
            </div>
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Title Template</label
                >
                <p class="text-xs text-gray-400 mb-1">
                    Use
                    <code class="text-blue-500 bg-blue-50 px-1 rounded"
                        >{<!-- -->{fieldName}}</code
                    >
                    to include field values.
                </p>
                <TemplateInput
                    :model-value="titleTemplate"
                    @update:model-value="(v) => onChange('titleTemplate', v)"
                    :fields="fields"
                    placeholder="e.g. {{ description }} — {{ status }}"
                />
            </div>
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Display Fields
                </label>
                <div
                    class="space-y-1 max-h-48 overflow-y-auto border border-gray-200 rounded-md p-2"
                >
                    <label
                        v-for="f in fields"
                        :key="f.name"
                        class="flex items-center gap-2 px-2 py-1 rounded hover:bg-gray-50 cursor-pointer text-sm"
                    >
                        <Checkbox
                            :binary="true"
                            :model-value="isDisplayField(f.name)"
                            @change="toggleDisplay(f.name)"
                        />
                        <span class="text-gray-700"
                            ><FieldNameLabel :field="f"
                        /></span>
                    </label>
                </div>
            </div>
        </div>
    </div>
</template>
