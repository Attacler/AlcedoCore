<script setup lang="ts">
import { useToast } from "@/composables/useToast";
import { useCollectionsStore } from "@/stores/collections";
import {
    PolicyPermission,
    PolicyWithPermissions,
    usePoliciesStore,
} from "@/stores/policies";
import { FilterCondition } from "@/types/filters";
import { Button, Drawer, MultiSelect, RadioButton, Select } from "primevue";
import { watch } from "vue";
import { computed, ref } from "vue";
import FilterBuilder from "../FilterBuilder.vue";
import {
    SYSTEM_FIELD_LABELS,
    SYSTEM_FIELD_NAMES,
} from "@/composables/useSystemFields.ts";

const props = defineProps<{
        permissions: PolicyPermission[];
        policy: PolicyWithPermissions;
    }>(),
    emit = defineEmits(["reload"]);

const store = usePoliciesStore(),
    toast = useToast(),
    collectionsStore = useCollectionsStore();

const ruleForm = ref<{
        collection_name: string | null;
        action: string | null;
        fields: string[] | null;
        filter: Array<{ field: string; operator: string; value: string }>;
        field_validation: Array<{
            field: string;
            operator: string;
            value: string;
        }>;
    }>({
        collection_name: null,
        action: null,
        fields: null,
        filter: [],
        field_validation: [],
    }),
    savingRule = ref(false),
    showRuleDialog = ref(false),
    editingRule = ref(false),
    editingRuleId = ref<string | null>(null),
    filterCondition = ref<FilterCondition | null>(null),
    validationCondition = ref<FilterCondition | null>(null);

const filterConditionKey = computed(() =>
    JSON.stringify(filterCondition.value),
);

watch(filterConditionKey, () => {
    if (!filterCondition.value) {
        ruleForm.value.filter = [];
        return;
    }
    const result: Array<{ field: string; operator: string; value: string }> =
        [];
    function walk(c: FilterCondition) {
        if ("field" in c) {
            result.push({
                field: c.field,
                operator: c.operator,
                value:
                    c.value !== null && c.value !== undefined
                        ? String(c.value)
                        : "",
            });
        } else {
            for (const child of c.conditions) {
                walk(child);
            }
        }
    }
    walk(filterCondition.value);
    ruleForm.value.filter = result;
});
const selectedCollection = computed(() => {
    if (!ruleForm.value.collection_name) return null;
    return (
        collectionsStore.collections.find(
            (c) => c.name === ruleForm.value.collection_name,
        ) || null
    );
});

const actionOptions = [
    { label: "Create", value: "create" },
    { label: "Read", value: "read" },
    { label: "Update", value: "update" },
    { label: "Delete", value: "delete" },
];

const availableFields = computed(() => {
    if (!ruleForm.value.collection_name) return [];
    const collection = collectionsStore.collections.find(
        (c) => c.name === ruleForm.value.collection_name,
    );
    if (!collection) return [];

    return [
        ...collection.fields.map((f) => ({
            label: `${f.display_name || f.name} (${f.type})`,
            value: f.name,
        })),
        ...SYSTEM_FIELD_NAMES.map((e) => ({
            label: SYSTEM_FIELD_LABELS[e],
            value: e,
        })),
    ];
});

const collectionOptions = computed(() => {
    return [
        ...collectionsStore.collections.map((c) => ({
            label: c.display_name || c.name,
            value: c.name,
        })),
        ...SYSTEM_FIELD_NAMES.map((e) => ({
            label: SYSTEM_FIELD_LABELS[e],
            value: e,
        })),
    ];
});

function onCollectionChange() {
    ruleForm.value.fields = null;
    ruleForm.value.action = null;
    filterCondition.value = null;
    validationCondition.value = null;
}

async function saveRule() {
    if (
        !ruleForm.value.collection_name ||
        !ruleForm.value.action ||
        savingRule.value
    )
        return;
    savingRule.value = true;
    try {
        const data = {
            collection_name: ruleForm.value.collection_name,
            action: ruleForm.value.action,
            fields: ruleForm.value.fields,
            filter: ruleForm.value.filter.filter((f) => f.field.trim()),
            field_validation: ruleForm.value.field_validation.filter((f) =>
                f.field.trim(),
            ),
        };

        if (editingRule.value && editingRuleId.value) {
            await store.updatePermission(props.policy.id, editingRuleId.value, {
                action: data.action,
                fields: data.fields,
                filter: data.filter,
                field_validation: data.field_validation,
            });
        } else {
            await store.createPermission(props.policy.id, data);
        }
        emit("reload");
        closeRuleDialog();
        toast.show(
            editingRule.value ? "Rule updated" : "Rule added",
            "success",
        );
    } catch (e) {
        toast.show(
            `Failed to save rule: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
        console.error(e);
    } finally {
        savingRule.value = false;
    }
}

function closeRuleDialog() {
    showRuleDialog.value = false;
    editingRule.value = false;
    editingRuleId.value = null;
    filterCondition.value = null;
    validationCondition.value = null;
}

function openAddRuleDialog(collection?: string, action?: string) {
    editingRule.value = false;
    editingRuleId.value = null;
    ruleForm.value = {
        collection_name: collection || null,
        action: action || null,
        fields: null,
        filter: [],
        field_validation: [],
    };
    filterCondition.value = null;
    validationCondition.value = null;
    showRuleDialog.value = true;
}

function filtersToCondition(
    filters: Array<{ field: string; operator: string; value: string }>,
): FilterCondition | null {
    const valid = filters.filter((f) => f.field.trim());
    if (valid.length === 0) return null;
    if (valid.length === 1) {
        return {
            field: valid[0].field,
            operator: valid[0].operator as any,
            value: valid[0].value,
        };
    }
    return {
        operator: "and",
        conditions: valid.map((f) => ({
            field: f.field,
            operator: f.operator as any,
            value: f.value,
        })),
    };
}

function openEditRuleDialog(rule: PolicyPermission) {
    editingRule.value = true;
    editingRuleId.value = rule.id;
    const filters = (rule.filter || []).map((f: any) => ({
        field: f.field || "",
        operator: f.operator || "eq",
        value: f.value !== undefined ? String(f.value) : "",
    }));
    const fieldValidations = (rule.field_validation || []).map((f: any) => ({
        field: f.field || "",
        operator: f.operator || "eq",
        value: f.value !== undefined ? String(f.value) : "",
    }));
    ruleForm.value = {
        collection_name: rule.collection_name,
        action: rule.action,
        fields: rule.fields ? [...rule.fields] : null,
        filter: filters,
        field_validation: fieldValidations,
    };
    filterCondition.value = filtersToCondition(filters);
    validationCondition.value = filtersToCondition(fieldValidations);
    showRuleDialog.value = true;
}

const validationConditionKey = computed(() =>
    JSON.stringify(validationCondition.value),
);
watch(validationConditionKey, () => {
    if (!validationCondition.value) {
        ruleForm.value.field_validation = [];
        return;
    }
    const result: Array<{ field: string; operator: string; value: string }> =
        [];
    function walk(c: FilterCondition) {
        if ("field" in c) {
            result.push({
                field: c.field,
                operator: c.operator,
                value:
                    c.value !== null && c.value !== undefined
                        ? String(c.value)
                        : "",
            });
        } else {
            for (const child of c.conditions) {
                walk(child);
            }
        }
    }
    walk(validationCondition.value);
    ruleForm.value.field_validation = result;
});

defineExpose({
    openAddRuleDialog,
    openEditRuleDialog,
});
</script>

<template>
    <Drawer
        v-model:visible="showRuleDialog"
        :header="editingRule ? 'Edit Rule' : 'Add Rule'"
        position="right"
        class="w-full! md:w-1/3!"
    >
        <form @submit.prevent="saveRule" class="space-y-4">
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1"
                    >Collection</label
                >
                <div v-if="editingRule" class="font-bold">
                    {{ ruleForm.collection_name }}
                </div>
                <Select
                    v-model="ruleForm.collection_name"
                    :options="collectionOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="Select a collection"
                    class="w-full"
                    fluid
                    @change="onCollectionChange"
                    v-else
                />
            </div>
            <div>
                <label class="block text-sm font-medium text-gray-700 mb-1"
                    >Action</label
                >
                <div v-if="editingRule" class="font-bold">
                    {{ ruleForm.action }}
                </div>
                <div class="flex gap-4" v-else>
                    <div
                        v-for="action in actionOptions"
                        :key="action.value"
                        class="flex items-center gap-1"
                    >
                        <RadioButton
                            :id="'action-' + action.value"
                            v-model="ruleForm.action"
                            :value="action.value"
                        />
                        <label
                            :for="'action-' + action.value"
                            class="text-sm text-gray-700"
                            >{{ action.label }}</label
                        >
                    </div>
                </div>
            </div>
            <div v-if="ruleForm.action === 'read'">
                <label class="block text-sm font-medium text-gray-700 mb-1">
                    Fields
                    <span class="text-xs font-normal text-gray-400 ml-1"
                        >(leave empty for all fields)</span
                    >
                </label>
                <MultiSelect
                    v-model="ruleForm.fields"
                    :options="availableFields"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All fields"
                    class="w-full"
                    fluid
                    :showToggleAll="false"
                />
            </div>
            <div v-if="ruleForm.action === 'create'">
                <label class="block text-sm font-medium text-gray-700 mb-1">
                    Fields the plugin can set
                    <span class="text-xs font-normal text-gray-400 ml-1"
                        >(leave empty for all fields)</span
                    >
                </label>
                <MultiSelect
                    v-model="ruleForm.fields"
                    :options="availableFields"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All fields"
                    class="w-full"
                    fluid
                    :showToggleAll="false"
                />
            </div>
            <div v-if="ruleForm.action === 'update'">
                <label class="block text-sm font-medium text-gray-700 mb-1">
                    Fields the plugin can change
                    <span class="text-xs font-normal text-gray-400 ml-1"
                        >(leave empty for all fields)</span
                    >
                </label>
                <MultiSelect
                    v-model="ruleForm.fields"
                    :options="availableFields"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="All fields"
                    class="w-full"
                    fluid
                    :showToggleAll="false"
                />
            </div>
            <div
                v-if="
                    ruleForm.action === 'read' ||
                    ruleForm.action === 'update' ||
                    ruleForm.action === 'delete'
                "
            >
                <label class="block text-sm font-medium text-gray-700 mb-1"
                    >Row Filter</label
                >
                <FilterBuilder
                    v-if="selectedCollection"
                    v-model="filterCondition"
                    :fields="selectedCollection.fields"
                    :collection-name="ruleForm.collection_name || ''"
                    :related-field-options="[]"
                />
                <div
                    v-else
                    class="text-sm text-gray-400 italic py-4 text-center"
                >
                    Select a collection to configure filters
                </div>
            </div>
            <div v-if="ruleForm.action === 'create'">
                <label class="block text-sm font-medium text-gray-700 mb-1"
                    >Field Validation (allowed values)</label
                >
                <FilterBuilder
                    v-if="selectedCollection"
                    v-model="validationCondition"
                    :fields="selectedCollection.fields"
                    :collection-name="ruleForm.collection_name || ''"
                    :related-field-options="[]"
                />
                <div
                    v-else
                    class="text-sm text-gray-400 italic py-4 text-center"
                >
                    Select a collection to configure field validation
                </div>
            </div>
            <div v-if="ruleForm.action === 'update'">
                <label class="block text-sm font-medium text-gray-700 mb-1"
                    >Field Validation (allowed values)</label
                >
                <FilterBuilder
                    v-if="selectedCollection"
                    v-model="validationCondition"
                    :fields="selectedCollection.fields"
                    :collection-name="ruleForm.collection_name || ''"
                    :related-field-options="[]"
                />
                <div
                    v-else
                    class="text-sm text-gray-400 italic py-4 text-center"
                >
                    Select a collection to configure field validation
                </div>
            </div>
            <div v-if="ruleForm.action === 'delete'">
                <p class="text-sm text-gray-500 italic">
                    Only row filter applies to delete permissions.
                </p>
            </div>
            <div class="flex gap-3">
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="closeRuleDialog"
                />
                <Button
                    :label="editingRule ? 'Save' : 'Add'"
                    severity="primary"
                    :disabled="
                        !ruleForm.collection_name ||
                        !ruleForm.action ||
                        savingRule
                    "
                    @click="saveRule"
                />
            </div>
        </form>
    </Drawer>
</template>
