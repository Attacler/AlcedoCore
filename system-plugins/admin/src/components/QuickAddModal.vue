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

function onVisibleChange(val: boolean) {
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
        // Merge inlined O2M children into the same request (parent + children, atomic).
        let inlinedTempIds: string[] = [];
        if (
            recordFormRef.value &&
            typeof recordFormRef.value.getCreateBody === "function"
        ) {
            const { body, inlinedTempIds: ids } =
                recordFormRef.value.getCreateBody();
            if (body) Object.assign(payload, body);
            inlinedTempIds = ids;
        }
        const res = (await client.items.create(
            props.collectionName,
            payload,
        )) as any;
        const createdRaw = res.created || res.data || res;
        const created = Array.isArray(createdRaw) ? createdRaw[0] : createdRaw;
        const createdId = created?.id ?? null;
        // Drop inlined children, then flush any remaining queued ops (nested ones).
        if (recordFormRef.value) {
            if (
                typeof recordFormRef.value.consumeInlinedCreates === "function"
            ) {
                recordFormRef.value.consumeInlinedCreates(inlinedTempIds);
            }
            if (
                createdId &&
                typeof recordFormRef.value.flushPendingChildren === "function"
            ) {
                await recordFormRef.value.flushPendingChildren(createdId);
            }
        }
        toast.show("Item created successfully", "success");
        emit("created", created);
        visibleInner.value = false;
        emit("update:visible", false);
    } catch (e) {
        toast.show(
            e instanceof Error ? e.message : "Failed to create item",
            "error",
        );
    } finally {
        saving.value = false;
    }
}

function close() {
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
