<script setup lang="ts">
import { DatePicker } from "primevue";
import { computed } from "vue";

const props = withDefaults(
    defineProps<{
        field?: any;
        modelValue?: string | Date | null;
        invalid?: boolean | string;
        readonly?: boolean;
    }>(),
    {
        modelValue: null,
    },
);

const emit = defineEmits<{
    "update:modelValue": [value: string | null];
}>();

const dateValue = computed(() => {
    const v = props.modelValue;
    if (v == null || v === "") return null;
    return v instanceof Date ? v : new Date(v);
});

function onUpdate(value: Date | null) {
    emit("update:modelValue", value ? value.toISOString() : null);
}
</script>

<template>
    <DatePicker
        :modelValue="dateValue"
        @update:modelValue="onUpdate"
        :invalid="!!invalid"
        :disabled="readonly"
        hour-format="24"
        fluid
        class="text-sm"
    />
</template>
