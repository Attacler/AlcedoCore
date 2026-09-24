<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useCollectionsStore } from "@/stores/collections";
import { useAppContextStore } from "@/stores/appContext";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import RecordForm from "@/components/RecordForm.vue";

const props = defineProps<{
    visible: boolean;
    collectionName: string;
    relatedApp?: string;
    deferred?: boolean;
}>();
const emit = defineEmits<{
    "update:visible": [value: boolean];
    created: [item: any];
}>();

const collectionsStore = useCollectionsStore(),
    appContext = useAppContextStore(),
    { client } = useAlcedoClient(),
    toast = useToast();

const currentVersion = computed(() => appContext.version ?? undefined);

const visibleInner = ref(props.visible);
const formValues = ref<Record<string, any>>({});
const saving = ref(false);
const loadingFields = ref(false);
const fetchError = ref<string | null>(null);
const recordFormRef = ref<any>(null);

watch(
    () => props.visible,
    (val) => {
        visibleInner.value = val;
        if (val) loadFields();
    },
);
function hasPendingChanges(): boolean {
    return !!(
        recordFormRef.value &&
        typeof recordFormRef.value.hasPendingChanges === "function" &&
        recordFormRef.value.hasPendingChanges()
    );
}

function onVisibleChange(val: boolean) {
    if (!val && hasPendingChanges()) {
        if (!window.confirm("Discard unsaved changes?")) return;
    }
    visibleInner.value = val;
    emit("update:visible", val);
}

async function loadFields() {
    loadingFields.value = true;
    fetchError.value = null;
    try {
        await collectionsStore.getCollection(props.collectionName, true, {
            app: props.relatedApp ?? undefined,
            version: currentVersion.value,
        });
        formValues.value = {};
    } catch (e) {
        fetchError.value =
            e instanceof Error ? e.message : "Failed to load collection";
    } finally {
        loadingFields.value = false;
    }
}

async function save() {
    if (recordFormRef.value && !recordFormRef.value.validate()) return;
    saving.value = true;
    try {
        // In deferred mode, do not persist the record — hand the raw values back
        // so the parent form can save them together with its own record.
        if (props.deferred) {
            const payload: Record<string, any> = {};
            for (const [key, value] of Object.entries(formValues.value)) {
                if (value === null || value === undefined || value === "")
                    continue;
                payload[key] =
                    value instanceof Date ? value.toISOString() : value;
            }
            emit("created", payload);
            visibleInner.value = false;
            emit("update:visible", false);
            return;
        }

        const payload: Record<string, any> = {};
        for (const [key, value] of Object.entries(formValues.value)) {
            if (value === null || value === undefined || value === "") continue;
            payload[key] = value instanceof Date ? value.toISOString() : value;
        }
        // Merge nested relational sections into the same request (atomic).
        if (
            recordFormRef.value &&
            typeof recordFormRef.value.getRelationBody === "function"
        ) {
            const body = recordFormRef.value.getRelationBody();
            if (body) Object.assign(payload, body);
        }
        const res = await client.items.create(props.collectionName, payload, {
            app: props.relatedApp ?? undefined,
            version: currentVersion.value,
        });
        const createdRaw = res.created || res.data || res;
        const created = Array.isArray(createdRaw) ? createdRaw : [createdRaw];
        const item = created[0];
        toast.show("Item created successfully", "success");
        emit("created", item);
        visibleInner.value = false;
        emit("update:visible", false);
    } catch (e: any) {
        toast.show(
            e.data?.detail || e.message || "Failed to create item",
            "error",
        );
    } finally {
        saving.value = false;
    }
}
function close() {
    if (hasPendingChanges() && !window.confirm("Discard unsaved changes?"))
        return;
    visibleInner.value = false;
    emit("update:visible", false);
}
</script>

<template>
    <Drawer
        v-model:visible="visibleInner"
        :header="`Add ${collectionName}`"
        :modal="true"
        :style="{ width: '640px' }"
        :draggable="false"
        :closable="!saving"
        position="right"
        @update:visible="onVisibleChange"
    >
        <div v-if="loadingFields" class="text-center py-8 text-gray-500">
            Loading...
        </div>
        <template v-else>
            <div v-if="fetchError" class="text-red-500 text-sm mb-4">
                {{ fetchError }}
            </div>
            <div v-else class="space-y-3">
                <RecordForm
                    ref="recordFormRef"
                    :collection-name="collectionName"
                    v-model="formValues"
                    :scalar-only="deferred"
                    :target-app="relatedApp"
                    :target-version="currentVersion"
                />
            </div>
        </template>
        <template #footer>
            <div class="flex gap-2 justify-end">
                <Button
                    label="Cancel"
                    severity="secondary"
                    :disabled="saving"
                    @click="close"
                />
                <Button label="Save" :loading="saving" @click="save" />
            </div>
        </template>
    </Drawer>
</template>
