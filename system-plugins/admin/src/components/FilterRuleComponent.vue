<script setup lang="ts">
import { computed, ref, watch, onErrorCaptured } from "vue";
import {
    useCollectionsStore,
    type FieldDefinition,
} from "@/stores/collections";
import type { FilterRule, OperatorMeta } from "@/types/filters";
import { OPERATORS_BY_TYPE } from "@/types/filters";
import { useAppContextStore } from "@/stores/appContext";
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

const collectionStore = useCollectionsStore(),
    appContext = useAppContextStore();

const currentApp = computed(() => appContext.appSlug ?? undefined),
    currentVersion = computed(() => appContext.version ?? undefined);

const MAX_TREE_DEPTH = 3;

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

interface CollectionRef {
    collection: string;
    app?: string;
}

const keyTypeMap = new Map<string, string>(),
    keyCollectionMap = new Map<string, CollectionRef>(),
    fieldTree = ref<FieldTreeNode[]>([]),
    treeReady = ref(false),
    selectedKeys = ref<Record<string, boolean> | null>(null),
    rawValue = ref(false);

const resolvedCollections = new Map<string, Promise<any>>();

function getCollectionCached(
    name: string,
    app: string | undefined,
): Promise<any> {
    const key = `${app ?? "__current__"}:${name}`;
    if (!resolvedCollections.has(key)) {
        resolvedCollections.set(
            key,
            collectionStore
                .getCollection(name, false, {
                    app: app ?? undefined,
                    version: currentVersion.value,
                })
                .catch(() => null),
        );
    }
    return resolvedCollections.get(key)!;
}

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

/** The selected field path joined into a tree key (e.g. "customer.name"). */
const dottedPath = computed(() => (props.condition.path || []).join("."));

/** Watch parent field reset via condition prop */
watch(
    () => props.condition.path,
    (val) => {
        const key = (val || []).join(".");
        selectedKeys.value = key ? { [key]: true } : null;
    },
    {
        immediate: true,
        deep: true,
    },
);

// Also emit up when user selects in the tree
watch(
    selectedKeys,
    (keys) => {
        if (!keys) return;
        const field = Object.keys(keys)[0];
        if (!field || field === dottedPath.value) return;
        const clone = getClone();
        clone.path = field.split(".");
        clone.operator = "eq";
        clone.value = "";
        emitUpdate(clone);
    },
    { deep: true },
);

interface TreeBuildResult {
    tree: FieldTreeNode[];
    mapping: Map<string, CollectionRef>;
}

function prefixTreeNodes(
    nodes: FieldTreeNode[],
    prefix: string,
): FieldTreeNode[] {
    return nodes.map((n) => ({
        ...n,
        key: `${prefix}.${n.key}`,
        children: n.children
            ? prefixTreeNodes(n.children, prefix)
            : undefined,
    }));
}

async function buildFullTree(
    fields: FieldDefinition[],
    collectionName: string,
    collectionApp: string | undefined,
    visited: Set<string>,
    depth: number,
): Promise<TreeBuildResult> {
    const tree: FieldTreeNode[] = [];
    const mapping = new Map<string, CollectionRef>();

    for (const f of fields) {
        if (f.type !== "relationship") {
            keyTypeMap.set(f.name, f.type);
            mapping.set(f.name, { collection: collectionName, app: collectionApp });
            tree.push({
                key: f.name,
                label: f.display_name || f.name,
                type: f.type,
                leaf: true,
            });
            continue;
        }

        if (!f.related_collection) continue;

        // Guard against bidirectional relations (e.g. orders.items ->
        // order_items.order -> orders) which would recurse forever.
        if (visited.has(f.related_collection)) continue;

        const relatedApp = f.related_app ?? collectionApp;
        const childColl = await getCollectionCached(
            f.related_collection,
            relatedApp ?? undefined,
        );
        const childFields: FieldDefinition[] | undefined = childColl?.fields;
        if (!childFields) continue;

        mapping.set(f.name, { collection: collectionName, app: collectionApp });

        const nextVisited = new Set(visited);
        nextVisited.add(f.related_collection);
        const children: FieldTreeNode[] = [];

        for (const cf of childFields) {
            const key = `${f.name}.${cf.name}`;
            keyTypeMap.set(key, cf.type);
            mapping.set(key, {
                collection: f.related_collection,
                app: relatedApp ?? undefined,
            });

            let grandChildren: FieldTreeNode[] = [];
            if (
                cf.type === "relationship" &&
                cf.related_collection &&
                !nextVisited.has(cf.related_collection) &&
                depth + 1 < MAX_TREE_DEPTH
            ) {
                const nestedApp = cf.related_app ?? relatedApp;
                const nestedColl = await getCollectionCached(
                    cf.related_collection,
                    nestedApp ?? undefined,
                );
                const sub = await buildFullTree(
                    nestedColl?.fields || [],
                    cf.related_collection,
                    nestedApp ?? undefined,
                    nextVisited,
                    depth + 1,
                );
                grandChildren = prefixTreeNodes(sub.tree, key);
                for (const [subKey, info] of sub.mapping) {
                    const fullKey = `${key}.${subKey}`;
                    mapping.set(fullKey, info);
                    const subType = keyTypeMap.get(subKey);
                    if (subType) keyTypeMap.set(fullKey, subType);
                }
            }

            children.push({
                key,
                label: `${f.display_name || f.name} > ${cf.display_name || cf.name}`,
                type: cf.type,
                leaf: true,
                children: grandChildren,
            });
        }

        tree.push({
            key: f.name,
            label: `${f.display_name || f.name} (${f.related_collection})`,
            leaf: false,
            children,
        });
    }

    return { tree, mapping };
}

let buildToken = 0;

async function rebuildTree() {
    const token = ++buildToken;
    keyTypeMap.clear();
    keyCollectionMap.clear();
    if (!props.fields || props.fields.length === 0) {
        fieldTree.value = [];
        treeReady.value = true;
        return;
    }
    const { tree, mapping } = await buildFullTree(
        props.fields,
        props.collectionName,
        currentApp.value,
        new Set([props.collectionName]),
        0,
    );
    if (token !== buildToken) return;
    for (const [key, info] of mapping) {
        keyCollectionMap.set(key, info);
    }
    fieldTree.value = tree;
    treeReady.value = true;
}

watch(
    () => [props.fields, props.collectionName, currentApp.value],
    rebuildTree,
    { immediate: true },
);

const currentKey = computed(() => {
    if (!selectedKeys.value) return null;
    return Object.keys(selectedKeys.value)[0];
});

const selectedFieldType = computed<string>(() => {
    const fieldName = (props.condition.path || []).join(".");
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

function getCollectionInfo(key: string): CollectionRef {
    return (
        keyCollectionMap.get(key) ?? {
            collection: props.collectionName,
            app: currentApp.value,
        }
    );
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
                    :collectionName="getCollectionInfo(currentKey).collection"
                    :fieldName="currentKey.split('.').reverse()[0]"
                    :targetApp="getCollectionInfo(currentKey).app"
                    :targetVersion="currentVersion"
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
