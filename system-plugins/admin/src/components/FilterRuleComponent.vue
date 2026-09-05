<script setup lang="ts">
import { computed, ref, watch, onErrorCaptured } from "vue";
import {
    useCollectionsStore,
    type FieldDefinition,
} from "@/stores/collections";
import type { FilterRule, OperatorMeta } from "@/types/filters";
import { OPERATORS_BY_TYPE } from "@/types/filters";
import Select from "primevue/select";
import TreeSelect from "primevue/treeselect";
import Button from "primevue/button";
import FormFieldRenderer from "./FormFieldRenderer.vue";

const props = defineProps<{
        condition: FilterRule;
        fields: FieldDefinition[];
        collectionName: string;
    }>(),
    emit = defineEmits<{
        "update:condition": [value: FilterRule];
        remove: [];
    }>();

const collectionStore = useCollectionsStore();

onErrorCaptured((err: any) => {
    console.error("[FilterRuleComponent] Error:", err?.message, err?.stack);
    return false;
});

interface FieldTreeNode {
    key: string;
    label: string;
    type?: string;
    leaf?: boolean;
    children?: FieldTreeNode[];
}

const keyTypeMap = new Map<string, string>(),
    fieldTree = ref<FieldTreeNode[]>([]),
    treeReady = ref(false),
    selectedKeys = ref<Record<string, boolean> | null>(null),
    rawValue = ref(false);

/** Recursively find a tree node by key */
function findNodeByKey(
    nodes: FieldTreeNode[],
    key: string,
): FieldTreeNode | undefined {
    for (const n of nodes) {
        if (n.key === key) return n;
        if (n.children) {
            const found = findNodeByKey(n.children, key);
            if (found) return found;
        }
    }
}

/** Display label for the selected field — walks fieldTree */
const selectedLabel = computed(() => {
    if (!selectedKeys.value) return "";
    const key = Object.keys(selectedKeys.value)[0];
    if (!key) return "";
    const node = findNodeByKey(fieldTree.value, key);
    return node ? node.label : key;
});

/** Watch parent field reset via condition prop */
watch(
    () => props.condition.field,
    (val) => {
        selectedKeys.value = val ? { [val]: true } : null;
    },
    {
        immediate: true,
    },
);

// Also emit up when user selects in the tree
watch(
    selectedKeys,
    (keys) => {
        if (!keys) return;
        const field = Object.keys(keys)[0];
        if (!field || field === props.condition.field) return;
        const clone = getClone();
        clone.field = field;
        clone.operator = "eq";
        clone.value = "";
        emitUpdate(clone);
    },
    { deep: true },
);

function buildFullTree(fields: FieldDefinition[]): FieldTreeNode[] {
    const tree: FieldTreeNode[] = [];

    for (const f of fields) {
        if (f.type !== "relationship") {
            keyTypeMap.set(f.name, f.type);
            tree.push({
                key: f.name,
                label: f.display_name || f.name,
                type: f.type,
                leaf: true,
            });
        } else if (f.related_collection) {
            const childFields = collectionStore.collections.find(
                (e) => e.name == f.related_collection,
            )?.fields;

            if (childFields) {
                const children: FieldTreeNode[] = childFields.map((cf) => {
                    const key = `${f.name}.${cf.name}`;
                    keyTypeMap.set(key, cf.type);

                    const children = cf.related_collection
                        ? buildFullTree(
                              collectionStore.collections.find(
                                  (e) => e.name == cf.related_collection,
                              )?.fields || [],
                          ).map((e) => ({
                              ...e,
                              key: key + "." + e.key,
                          }))
                        : [];

                    return {
                        key,
                        label: `${f.display_name || f.name} > ${cf.display_name || cf.name}`,
                        type: cf.type,
                        leaf: true,
                        children,
                    };
                });
                tree.push({
                    key: f.name,
                    label: `${f.display_name || f.name} (${f.related_collection})`,
                    leaf: false,
                    children,
                });
            }
        }
    }

    fieldTree.value = tree;
    treeReady.value = true;

    return tree;
}

if (props.fields.length > 0) buildFullTree(props.fields);

const currentKey = computed(() => {
    if (!selectedKeys.value) return null;
    return Object.keys(selectedKeys.value)[0];
});

const selectedFieldType = computed<string>(() => {
    const fieldName = props.condition.field;
    if (!fieldName) return "string";
    return keyTypeMap.get(fieldName) || "string";
});

const availableOperators = computed<OperatorMeta[]>(() => {
    const type = selectedFieldType.value;
    return OPERATORS_BY_TYPE[type] || OPERATORS_BY_TYPE.string;
});

const requiresValue = computed(() => {
    const op = availableOperators.value.find(
        (o) => o.operator === props.condition.operator,
    );
    return op ? op.requiresValue : true;
});

function getClone(): FilterRule {
    return { ...props.condition };
}

function emitUpdate(clone: FilterRule) {
    emit("update:condition", clone);
}

function onOperatorChange(event: any) {
    const val = event?.value ?? event;
    const clone = getClone();
    clone.operator = val;
    const op = availableOperators.value.find((o) => o.operator === val);
    clone.value = op?.requiresValue ? "" : null;
    emitUpdate(clone);
}

function onValueChange(newValue: unknown) {
    const clone = getClone();
    clone.value = newValue;
    emitUpdate(clone);
}

function getCollectionName(key: string) {
    if (!key.includes(".")) {
        return props.collectionName;
    }

    let lastCollectionName = props.collectionName;
    const keys = key.split(".");
    keys.pop();

    for (const k of keys) {
        const findCollection = collectionStore.collections.find(
            (e) => e.name == lastCollectionName,
        );

        const findField = findCollection?.fields.find((e) => e.name == k);

        if (findField?.related_collection) {
            lastCollectionName = findField.related_collection;
            continue;
        }

        return lastCollectionName;
    }

    return lastCollectionName;
}
</script>

<template>
    <div
        class="filter-rule flex flex-col sm:flex-row sm:items-start gap-1 sm:gap-2 p-2 bg-white border border-gray-200 rounded-md"
    >
        <div class="w-full sm:flex-1 min-w-0">
            <label
                class="block text-xs text-gray-500 mb-0.5 sm:mb-1 font-medium"
            >
                Field
            </label>

            <TreeSelect
                v-if="treeReady"
                v-model="selectedKeys"
                :options="fieldTree"
                selectionMode="single"
                placeholder="Select field..."
                class="w-full"
                scrollHeight="300px"
                filter
            >
                <template #value="slotProps">
                    <span v-if="selectedLabel">{{ selectedLabel }}</span>
                    <span v-else>{{ slotProps.placeholder }}</span>
                </template>
            </TreeSelect>
        </div>

        <div class="w-full sm:flex-1 min-w-0">
            <label
                class="block text-xs text-gray-500 mb-0.5 sm:mb-1 font-medium"
                >Operator</label
            >
            <Select
                :modelValue="condition.operator"
                :options="availableOperators"
                optionLabel="label"
                optionValue="operator"
                placeholder="Select operator..."
                class="w-full"
                @change="onOperatorChange"
            />
        </div>

        <div
            v-if="requiresValue && currentKey"
            class="w-full sm:flex-2 min-w-0"
        >
            <label
                class="block text-xs text-gray-500 mb-0.5 sm:mb-1 font-medium"
            >
                Value
            </label>
            <div class="flex items-center mb-1 gap-2">
                <FormFieldRenderer
                    v-if="!rawValue"
                    :collectionName="getCollectionName(currentKey)"
                    :fieldName="currentKey.split('.').reverse()[0]"
                    :modelValue="condition.value"
                    @update:modelValue="onValueChange"
                    class="grow"
                />
                <InputText
                    v-else
                    :modelValue="condition.value as string"
                    @update:modelValue="onValueChange"
                    fluid
                />
                <ToggleButton
                    onLabel="Raw value"
                    offLabel="Display"
                    size="small"
                    v-model="rawValue"
                    class="shrink-0"
                />
            </div>
        </div>

        <div class="filter-remove self-end sm:self-auto sm:pt-5 -mt-1 sm:mt-0">
            <Button
                icon="pi pi-times"
                severity="danger"
                text
                size="small"
                @click="emit('remove')"
                :title="'Remove filter'"
            />
        </div>
    </div>
</template>
