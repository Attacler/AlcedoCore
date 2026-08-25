<script setup lang="ts">
import { ref, computed, watchEffect } from "vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useCollectionsStore } from "@/stores/collections";
import Select from "primevue/select";
import QuickCreateDialog from "@/components/QuickCreateDialog.vue";

const props = withDefaults(
        defineProps<{
            field?: any;
            modelValue?: any;
            invalid?: boolean | string;
            readonly?: boolean;
            collectionName?: string;
            inlineCreate?: boolean;
        }>(),
        {
            field: undefined,
            modelValue: undefined,
            invalid: false,
            readonly: false,
            collectionName: "",
            inlineCreate: false,
        },
    ),
    emit = defineEmits<{
        "update:modelValue": [value: any];
    }>();

const { client } = useAlcedoClient(),
    collectionsStore = useCollectionsStore();

const relatedOptions = ref<{ label: string; value: string }[]>([]),
    relatedLoading = ref(false);

async function resolveDisplayField(): Promise<string | undefined> {
    const displayField = props.field?.display_field;
    if (displayField) return displayField;

    const relatedCollection = props.field?.related_collection;
    if (!relatedCollection) return undefined;
    try {
        const target = await collectionsStore.getCollection(relatedCollection);
        const fieldList: any[] = target.fields || [];
        const firstString = fieldList.find(
            (f: any) => f.type === "string" && !f.is_system,
        );
        const nameField = fieldList.find(
            (f: any) => f.name === "name" || f.name === "title",
        );
        const chosen = nameField || firstString;
        return chosen?.name;
    } catch {
        return undefined;
    }
}

async function loadRelatedOptions() {
    const relatedCollection = props.field?.related_collection;
    if (!relatedCollection) {
        relatedOptions.value = [];
        return;
    }
    relatedLoading.value = true;
    try {
        const displayField = await resolveDisplayField();
        const res = (await client.items.list(relatedCollection, {
            limit: "50",
        })) as any;
        const data = res.data || res;
        const items: any[] = data.data || data.items || data || [];
        relatedOptions.value = items.map((item: any) => ({
            label:
                displayField && item[displayField]
                    ? String(item[displayField])
                    : String(item.id),
            value: item.id,
        }));
    } catch {
        relatedOptions.value = [];
    } finally {
        relatedLoading.value = false;
    }
}

watchEffect(() => {
    if (
        props.field?.type !== "relationship" ||
        !props.field?.related_collection
    )
        return;
    loadRelatedOptions();
});

const showQuickCreate = ref(false),
    canCreateRelated = ref(false),
    pendingRelatedLabel = ref<string | null>(null);

const isPendingCreate = computed(() => {
    return (
        !!props.modelValue &&
        typeof props.modelValue === "object" &&
        !Array.isArray(props.modelValue)
    );
});

watchEffect(async () => {
    if (
        props.field?.type !== "relationship" ||
        !props.field?.related_collection ||
        props.readonly
    ) {
        canCreateRelated.value = false;
        return;
    }
    try {
        const policy = (await client.collections.getCreatePolicy(
            props.field.related_collection,
        )) as any;
        canCreateRelated.value = policy?.$permissions?.create !== false;
    } catch {
        canCreateRelated.value = false;
    }
});

// Keep the pending label in sync with the pending object (so the chip shows a
// human-readable label derived from the created record's display field).
watchEffect(async () => {
    if (!isPendingCreate.value) {
        pendingRelatedLabel.value = null;
        return;
    }
    const values = props.modelValue as Record<string, any>;
    const displayField = await resolveDisplayField();
    if (displayField && values[displayField]) {
        pendingRelatedLabel.value = String(values[displayField]);
    } else {
        pendingRelatedLabel.value = "New record";
    }
});

function openQuickCreate() {
    showQuickCreate.value = true;
}

function clearPendingCreate() {
    emit("update:modelValue", null);
}

async function onRelatedCreated(item: any) {
    if (!item) return;
    // Deferred mode: item is the raw values object (no id) → set as pending create.
    if (!item.id) {
        emit("update:modelValue", { ...item });
        return;
    }
    // Immediate mode: item is the created record → select it by id.
    emit("update:modelValue", item.id);
    await loadRelatedOptions();
}
</script>

<template>
    <div class="flex items-center gap-2">
        <!-- Pending inline-created related record (deferred until the parent saves) -->
        <div v-if="isPendingCreate" class="flex items-center gap-2 flex-1">
            <span
                class="inline-flex items-center gap-1.5 px-2 py-1 rounded-full bg-blue-50 text-blue-700 border border-blue-200 text-sm"
            >
                New record:
                {{ pendingRelatedLabel || "New record" }}
            </span>
            <Button
                icon="pi pi-times"
                severity="secondary"
                text
                rounded
                size="small"
                :title="`Remove new ${field?.related_collection}`"
                :aria-label="`Remove new ${field?.related_collection}`"
                @click="clearPendingCreate"
            />
        </div>

        <template v-else>
            <Select
                :modelValue="modelValue"
                @update:modelValue="emit('update:modelValue', $event)"
                :options="relatedOptions"
                option-label="label"
                option-value="value"
                :placeholder="
                    field?.related_collection
                        ? `Select ${field.related_collection}...`
                        : 'Lookup...'
                "
                :loading="relatedLoading"
                :invalid="!!invalid"
                :showClear="!field?.required && !readonly"
                :disabled="readonly"
                filter
                fluid
                class="text-sm flex-1"
            />
            <Button
                v-if="inlineCreate && canCreateRelated"
                icon="pi pi-plus"
                severity="secondary"
                outlined
                :title="`Create new ${field?.related_collection}`"
                :aria-label="`Create new ${field?.related_collection}`"
                @click="openQuickCreate"
            />
        </template>

        <QuickCreateDialog
            :visible="showQuickCreate"
            :collection-name="field?.related_collection"
            :deferred="inlineCreate"
            @update:visible="(v) => (showQuickCreate = v)"
            @created="onRelatedCreated"
        />
    </div>
</template>
