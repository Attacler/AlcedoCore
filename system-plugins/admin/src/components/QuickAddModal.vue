<script setup lang="ts">
import { ref, computed, watch } from "vue";
import type { FieldDefinition } from "@/stores/collections";
import {
    proposedValues,
    type FilterCondition,
    type ItemPermissions,
    type PermissionViolation,
} from "@alcedocore/sdk";
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
    saving = ref(false),
    permissions = ref<ItemPermissions | null>(null);
/**
 * `createPolicy.field_validation` may be the flat `{field, operator, value}`
 * shape `proposedValues` understands, or the mongo-style shorthand the API
 * actually stores. Only prefill when it is the former; otherwise skip.
 */
function isFlatFilter(value: unknown): value is FilterCondition[] {
    return (
        Array.isArray(value) &&
        value.every(
            (c) =>
                c !== null &&
                typeof c === "object" &&
                typeof (c as any).field === "string",
        )
    );
}

function prefillValues(): Record<string, any> {
    const validation = props.createPolicy?.field_validation;
    if (!isFlatFilter(validation)) return {};
    return proposedValues(validation);
}

/**
 * Fields the form may show. `allowed_fields` is the authoritative union; the
 * probe's accepted rules refine it. A field is settable only as part of a rule
 * the payload currently satisfies, so an empty rule-fields list means "any".
 */
const effectiveFields = computed<FieldDefinition[]>(() => {
    const base = props.fields || [];
    const rules = permissions.value?.rules;
    if (!rules || rules.length === 0) return base;
    // Union of the rules that accept: those are the shapes we can submit.
    const accepted = rules.filter((r) => r.allowed);
    const pool = accepted.length ? accepted : rules;
    if (pool.some((r) => r.fields.length === 0)) return base;
    const allowed = new Set(pool.flatMap((r) => r.fields));
    return base.filter((f) => allowed.has(f.name));
});

/** The exact object `save()` would POST — probe and POST must agree. */
function buildPayload(): Record<string, any> {
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
    return payload;
}

let probeTimer: ReturnType<typeof setTimeout> | undefined;

async function runProbe() {
    if (!props.collectionName) return;
    try {
        const result = await client.items.permissions(
            props.collectionName,
            buildPayload(),
        );
        permissions.value = result;
    } catch {
        // Never brick the form on a probe failure — let the server enforce.
        permissions.value = null;
    }
}

function scheduleProbe() {
    if (probeTimer) clearTimeout(probeTimer);
    probeTimer = setTimeout(runProbe, 500);
}

watch(
    () => props.visible,
    (val) => {
        visibleInner.value = val;
        if (val) {
            formValues.value = { ...prefillValues() };
            scheduleProbe();
        } else {
            if (probeTimer) clearTimeout(probeTimer);
            permissions.value = null;
        }
    },
);

watch(
    formValues,
    () => {
        if (visibleInner.value) scheduleProbe();
    },
    { deep: true },
);

const saveBlocked = computed(
    () => !!permissions.value && !permissions.value.allowed,
);

function describeViolation(v: PermissionViolation): string {
    if (v.reason === "not_permitted") return `${v.field} is not allowed`;
    if (v.reason === "relation_mismatch")
        return `the selected related record for ${v.field} does not satisfy this permission`;
    if (v.reason) return `${v.field} is not permitted (${v.reason})`;
    if (v.operator)
        return `${v.field} must be ${v.operator} ${JSON.stringify(v.expected)}`;
    return `${v.field} is not allowed`;
}

const denyReason = computed<string | null>(() => {
    const p = permissions.value;
    if (!p || p.allowed) return null;
    const reasons = [
        ...(p.violations || []).map(describeViolation),
        ...(p.unresolved || []).map(
            (u) => `waiting for a related selection for ${u.field}`,
        ),
    ];
    return reasons.length
        ? reasons.join("; ")
        : "This item cannot be created with the current values";
});

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

    saving.value = true;
    try {
        const payload = buildPayload();
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
                :fields-override="effectiveFields"
                :target-app="targetApp"
                :target-version="targetVersion"
            />
        </div>
        <template #footer>
            <div class="flex items-center gap-3 justify-end">
                <span
                    v-if="denyReason"
                    class="mr-auto text-xs text-red-500"
                >
                    {{ denyReason }}
                </span>
                <Button
                    label="Cancel"
                    severity="secondary"
                    :disabled="saving"
                    @click="close"
                />
                <Button
                    label="Save"
                    :loading="saving"
                    :disabled="saveBlocked"
                    @click="save"
                />
            </div>
        </template>
    </Drawer>
</template>
