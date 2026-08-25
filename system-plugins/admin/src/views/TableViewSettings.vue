<script setup lang="ts">
import { computed } from "vue";
import type { FieldDefinition } from "@/stores/collections";
import FieldNameLabel from "@/components/FieldNameLabel.vue";
import Checkbox from "primevue/checkbox";

const props = defineProps<{
    fields: FieldDefinition[];
    settings: Record<string, any>;
    onChange: (key: string, value: any) => void;
}>();

const selectedFields = computed(
    () => (props.settings?.displayFields as string[]) || [],
);

const selectedCount = computed(() => selectedFields.value.length);

function isSelected(name: string): boolean {
    return selectedFields.value.includes(name);
}

function toggle(name: string) {
    const current = selectedFields.value;
    const next = current.includes(name)
        ? current.filter((n) => n !== name)
        : [...current, name];

    props.onChange("displayFields", next);
}
</script>

<template>
    <div class="space-y-2">
        <h3 class="font-semibold text-gray-800 text-sm">Table Columns</h3>
        <p class="text-xs text-gray-400">
            Select which fields to show in the table view.
        </p>
        <div class="space-y-1 max-h-72 overflow-y-auto">
            <label
                v-for="f in fields"
                :key="f.name"
                class="flex items-center gap-2 px-2 py-1.5 rounded hover:bg-gray-50 cursor-pointer text-sm"
            >
                <Checkbox
                    :binary="true"
                    :model-value="isSelected(f.name)"
                    @change="toggle(f.name)"
                />
                <span class="text-gray-700"><FieldNameLabel :field="f" /></span>
                <span class="text-xs text-gray-400 ml-auto">{{ f.type }}</span>
            </label>
        </div>
        <p class="text-xs text-gray-400 pt-2 border-t border-gray-100">
            {{ selectedCount }} of {{ fields.length }} fields selected
        </p>
    </div>
</template>
