<script setup lang="ts">
import { ref, computed, watchEffect } from "vue";
import {
    useCollectionsStore,
    type FieldDefinition,
} from "@/stores/collections";
import { resolveInputForField } from "@/inputs";
import { useDisplayComponents } from "@/composables/useDisplayComponents";
import {
    isSystemFieldName,
    makeSystemField,
} from "@/composables/useSystemFields";
import { markRaw } from "vue";
import { useDevServerStore } from "@/stores/devServerStore";

const props = withDefaults(
    defineProps<{
        collectionName: string;
        fieldName: string;
        modelValue?: any;
        invalid?: boolean | string;
        readonly?: boolean;
        inlineCreate?: boolean;
        displayValue?: any;
    }>(),
    {
        modelValue: undefined,
        invalid: false,
        readonly: false,
        inlineCreate: false,
        displayValue: undefined,
    },
);

const emit = defineEmits<{
    "update:modelValue": [value: any];
}>();

const collectionsStore = useCollectionsStore(),
    devStore = useDevServerStore();

const loadingField = ref(false),
    fields = computed(() => {
        return (
            collectionsStore.collections.find(
                (c) => c.name == props.collectionName,
            )?.fields || []
        );
    });

const field = computed<FieldDefinition | undefined>(() => {
    if (!fields.value) return undefined;

    const found = fields.value.find((f) => f.name === props.fieldName);
    if (found) return found;
    return isSystemFieldName(props.fieldName)
        ? makeSystemField(props.fieldName)
        : undefined;
});

const inputComponent = computed(() => {
    if (!field.value) return null;
    return resolveInputForField(field.value);
});

const { getDisplayComponentForField } = useDisplayComponents();

const displayComponentProps = computed(() => {
    if (!field.value) return { value: props.modelValue };
    const isRelation = field.value.type === "relationship";

    return {
        value: props.modelValue,
        field: field.value,
        ...(isRelation
            ? {
                  "related-collection": field.value.related_collection,
                  "related-field": field.value.name,
                  "display-value": props.displayValue,
              }
            : {}),
    };
});
</script>

<template>
    <div class="space-y-1">
        <div v-if="loadingField" class="text-xs text-gray-400 italic">
            Loading field...
        </div>

        <div v-else-if="!field" class="text-xs text-gray-400 italic">
            Field "{{ fieldName }}" not found on "{{ collectionName }}"
        </div>

        <template v-else>
            <component
                v-if="!!props.readonly"
                :is="
                    markRaw(
                        devStore.displayComponents[field.display_component!] ||
                            getDisplayComponentForField(field),
                    )
                "
                v-bind="displayComponentProps"
            />
            <component
                v-else
                :is="
                    markRaw(
                        devStore.inputComponents[field.input_component!] ||
                            inputComponent!,
                    )
                "
                :modelValue="modelValue"
                @update:modelValue="emit('update:modelValue', $event)"
                :field="field"
                :collection-name="collectionName"
                :inline-create="inlineCreate"
                :invalid="invalid"
                :readonly="readonly"
            />
        </template>

        <p
            v-if="invalid && typeof invalid === 'string'"
            class="text-xs text-red-500"
        >
            {{ invalid }}
        </p>
    </div>
</template>
