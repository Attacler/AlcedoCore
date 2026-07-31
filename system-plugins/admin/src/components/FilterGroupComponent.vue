<script setup lang="ts">
import type { FieldDefinition } from "@/stores/collections";
import type {
    FilterCondition,
    FilterGroup,
    FilterRule,
    GroupOperator,
} from "@/types/filters";
import {
    isFilterRule,
    createEmptyRule,
    createEmptyGroup,
} from "@/types/filters";
import FilterRuleComponent from "./FilterRuleComponent.vue";
import type { RelatedFieldOption } from "./FilterBuilder.vue";

const props = defineProps<{
    condition: FilterGroup;
    fields: FieldDefinition[];
    collectionName: string;
    relatedFieldOptions?: RelatedFieldOption[];
    depth: number;
}>();

const emit = defineEmits<{
    "update:condition": [value: FilterGroup];
    remove: [];
}>();

const groupOperatorOptions = [
    { label: "AND", value: "and" as GroupOperator },
    { label: "OR", value: "or" as GroupOperator },
];

function isRule(cond: FilterCondition): boolean {
    return isFilterRule(cond);
}

function getClone(): FilterGroup {
    return JSON.parse(JSON.stringify(props.condition));
}

function emitUpdate(clone: FilterGroup) {
    emit("update:condition", clone);
}

function onOperatorChange(event: any) {
    const val = event?.value ?? event;
    const clone = getClone();
    clone.operator = val;
    emitUpdate(clone);
}

function addRule() {
    const clone = getClone();
    clone.conditions.push(createEmptyRule());
    emitUpdate(clone);
}

function addGroup() {
    const clone = getClone();
    clone.conditions.push(createEmptyGroup());
    emitUpdate(clone);
}

function onRuleUpdate(idx: number, updated: FilterRule) {
    const clone = getClone();
    clone.conditions[idx] = updated;
    emitUpdate(clone);
}

function onGroupUpdate(idx: number, updated: FilterGroup) {
    const clone = getClone();
    clone.conditions[idx] = updated;
    emitUpdate(clone);
}

function removeCondition(idx: number) {
    const clone = getClone();
    clone.conditions.splice(idx, 1);
    emitUpdate(clone);
}

function removeSelf() {
    emit("remove");
}
</script>

<template>
    <div
        class="filter-group"
        :class="[
            depth > 0
                ? 'border border-gray-200 rounded-md p-2 sm:p-3 bg-gray-50/50'
                : '',
        ]"
    >
        <!-- Group header: AND/OR toggle and controls -->
        <div class="filter-group-header flex items-center justify-between">
            <div class="flex items-center gap-2">
                <SelectButton
                    :modelValue="condition.operator"
                    :options="groupOperatorOptions"
                    optionLabel="label"
                    optionValue="value"
                    @change="onOperatorChange"
                    size="small"
                />
                <span class="text-xs text-gray-400 font-mono">group</span>
            </div>
            <div class="filter-group-actions flex items-center gap-1">
                <Button
                    icon="pi pi-plus"
                    label="Rule"
                    severity="secondary"
                    text
                    size="small"
                    @click="addRule"
                />
                <Button
                    icon="pi pi-folder-plus"
                    label="Group"
                    severity="secondary"
                    text
                    size="small"
                    @click="addGroup"
                />
                <Button
                    v-if="depth > 0"
                    icon="pi pi-trash"
                    severity="danger"
                    text
                    size="small"
                    @click="removeSelf"
                    :title="'Remove group'"
                />
            </div>
        </div>

        <!-- Conditions list -->
        <div class="filter-group-conditions space-y-2 mt-2">
            <div
                v-for="(cond, idx) in condition.conditions"
                :key="idx"
                class="filter-condition-item"
            >
                <!-- Connector label between conditions -->
                <div
                    v-if="idx > 0"
                    class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-gray-400 my-1"
                >
                    {{ condition.operator.toUpperCase() }}
                </div>

                <div class="flex items-start gap-2">
                    <!-- Rule -->
                    <FilterRuleComponent
                        v-if="isRule(cond)"
                        :condition="cond as FilterRule"
                        :fields="fields"
                        :collection-name="collectionName"
                        :related-field-options="relatedFieldOptions"
                        class="flex-1"
                        @update:condition="(r) => onRuleUpdate(idx, r)"
                        @remove="removeCondition(idx)"
                    />

                    <!-- Nested group (recursive) -->
                    <FilterGroupComponent
                        v-else
                        :condition="cond as FilterGroup"
                        :fields="fields"
                        :collection-name="collectionName"
                        :related-field-options="relatedFieldOptions"
                        :depth="depth + 1"
                        class="flex-1"
                        @update:condition="(g) => onGroupUpdate(idx, g)"
                        @remove="removeCondition(idx)"
                    />
                </div>
            </div>

            <!-- Empty conditions state -->
            <div
                v-if="condition.conditions.length === 0"
                class="text-center py-4 text-gray-400 text-xs border border-dashed border-gray-300 rounded-md"
            >
                No conditions — add a rule or group above
            </div>
        </div>
    </div>
</template>

<style scoped>
/* No scoped styles needed — all styling uses inline Tailwind classes */
</style>
