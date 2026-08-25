<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useCollectionsStore, type FieldType } from "@/stores/collections";
import Select from "primevue/select";
import Checkbox from "primevue/checkbox";
import FieldNameLabel from "@/components/FieldNameLabel.vue";

const props = defineProps<{
    field: any;
    collectionName: string;
}>();

const store = useCollectionsStore();

const relatedCollectionFields = ref<
    { name: string; display_name?: string; type: FieldType }[]
>([]);

const displayFieldOptions = computed(() =>
        relatedCollectionFields.value
            .filter((f) => f.type !== "relationship")
            .map((f) => ({ label: f.display_name || f.name, value: f.name })),
    ),
    inlineParentFieldOptions = computed(() =>
        relatedCollectionFields.value.filter((f) => f.type !== "relationship"),
    );

watch(
    () => props.field?.related_collection,
    async (rc) => {
        if (!rc) {
            relatedCollectionFields.value = [];
            return;
        }
        try {
            const c = await store.getCollection(rc);
            relatedCollectionFields.value = (c.fields || []).map((f: any) => ({
                name: f.name,
                display_name: f.display_name,
                type: f.type,
            }));
        } catch {
            relatedCollectionFields.value = [];
        }
    },
    { immediate: true },
);

function onRelatedCollectionChange(value: string) {
    if (!props.field) return;
    props.field.related_collection = value;
    props.field.display_field = undefined;
}

function onEditRelationType(value: string) {
    if (!props.field) return;
    props.field.relationship_type = value;
    if (value === "one_to_one") props.field.unique = true;
}

function isInlineParentFieldSelected(fieldName: string): boolean {
    const current = props.field?.inline_parent_fields;
    return Array.isArray(current) && current.includes(fieldName);
}

function toggleInlineParentField(fieldName: string) {
    if (!props.field) return;
    const current: string[] = props.field.inline_parent_fields || [];
    if (current.includes(fieldName)) {
        props.field.inline_parent_fields = current.filter(
            (f: string) => f !== fieldName,
        );
    } else {
        props.field.inline_parent_fields = [...current, fieldName];
    }
}
</script>

<template>
    <div v-if="field" class="space-y-3 pt-4 border-t border-gray-200">
        <h4 class="text-sm font-semibold text-gray-700">
            Relationship Settings
        </h4>
        <div>
            <label class="block text-xs font-medium text-gray-600 mb-1"
                >Related Collection</label
            >
            <Select
                :modelValue="field.related_collection"
                @update:modelValue="onRelatedCollectionChange"
                :options="store.collections"
                option-label="name"
                option-value="name"
                placeholder="Select..."
                class="w-full"
            />
        </div>
        <div>
            <label class="block text-xs font-medium text-gray-600 mb-1"
                >Relationship Type</label
            >
            <Select
                :modelValue="field.relationship_type || 'many_to_one'"
                @update:modelValue="onEditRelationType"
                :options="[
                    { label: 'Many to One (M:1)', value: 'many_to_one' },
                    { label: 'One to One (1:1)', value: 'one_to_one' },
                    { label: 'One to Many (1:M)', value: 'one_to_many' },
                ]"
                option-label="label"
                option-value="value"
                class="w-full"
            />
        </div>
        <div v-if="field.related_collection">
            <label class="block text-xs font-medium text-gray-600 mb-1">
                {{
                    field.relationship_type === "one_to_many"
                        ? "Display Field on Child"
                        : "Display Field"
                }}
            </label>
            <Select
                v-model="field.display_field"
                :options="displayFieldOptions"
                option-label="label"
                option-value="value"
                placeholder="None (show UUID)"
                :showClear="true"
                class="w-full"
            />
        </div>
        <div
            v-if="field.related_collection"
            class="pt-3 border-t border-gray-100"
        >
            <label class="block text-xs font-medium text-gray-600 mb-1"
                >Inline Parent Fields</label
            >
            <div class="space-y-1 max-h-32 overflow-y-auto">
                <label
                    v-for="f in inlineParentFieldOptions"
                    :key="f.name"
                    class="flex items-center gap-2 cursor-pointer"
                >
                    <Checkbox
                        :binary="true"
                        :modelValue="isInlineParentFieldSelected(f.name)"
                        @update:modelValue="toggleInlineParentField(f.name)"
                    />
                    <span class="text-sm text-gray-700">
                        <FieldNameLabel :field="f" />
                    </span>
                </label>
            </div>
            <p class="text-xs text-gray-400 mt-1">
                Show parent fields inline on child record detail
            </p>
        </div>
    </div>
</template>
