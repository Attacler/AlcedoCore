<script setup lang="ts">
import { ref, watch } from "vue";
import type { FieldDefinition } from "@/stores/collections";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import RecordForm from "@/components/RecordForm.vue";
import { Drawer } from "primevue";

const props = defineProps<{
    visible: boolean;
    collectionName: string;
    fields: FieldDefinition[];
    targetApp?: string;
    targetVersion?: string;
    createPolicy?: {
        allowed_fields: any[];
        field_validation: any[];
        $permissions: { create: boolean };
    } | null;
}>();

const emit = defineEmits<{
    "update:visible": [value: boolean];
    created: [item: any];
}>();

const { client } = useAlcedoClient(),
    toast = useToast();

const visibleInner = ref(props.visible),
    formValues = ref<Record<string, any>>({}),
    recordFormRef = ref<any>(null),
    saving = ref(false);

watch(
    () => props.visible,
    (val) => {
        visibleInner.value = val;
        if (val) {
            formValues.value = {};

            if (props.createPolicy?.field_validation) {
                const defaults: Record<string, any> = {};
                for (const rule of props.createPolicy.field_validation) {
                    if (
                        rule.operator === "eq" &&
                        !formValues.value[rule.field]
                    ) {
                        defaults[rule.field] = rule.value;
                    }
                }
                formValues.value = { ...defaults, ...formValues.value };
            }
        }
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

async function save() {
    if (recordFormRef.value && !recordFormRef.value.validate()) return;

    if (props.createPolicy?.field_validation) {
        for (const rule of props.createPolicy.field_validation) {
            const value = formValues.value[rule.field];
            if (rule.operator === "eq" && value !== rule.value) {
                toast.show(
                    `Field "${rule.field}" must be "${rule.value}"`,
                    "error",
                );
                return;
            }
        }
    }

    saving.value = true;
    try {
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
        const createOptions =
            props.targetApp || props.targetVersion
                ? { app: props.targetApp, version: props.targetVersion }
                : undefined;
        const res = await client.items.create(
            props.collectionName,
            payload,
            createOptions,
        );
        const createdRaw = res.created || res.data || res;
        const created = Array.isArray(createdRaw) ? createdRaw[0] : createdRaw;
        toast.show("Item created successfully", "success");
        emit("created", created);
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
        :header="`Add Item - ${collectionName}`"
        :closable="!saving"
        @update:visible="onVisibleChange"
        position="right"
        class="w-1/2!"
    >
        <div class="space-y-3">
            <RecordForm
                ref="recordFormRef"
                :collection-name="collectionName"
                v-model="formValues"
                :fields-override="fields"
                :target-app="targetApp"
                :target-version="targetVersion"
            />
        </div>
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
