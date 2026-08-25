<script setup lang="ts">
import { DatePicker, InputText } from "primevue";
import { ref } from "vue";

defineProps<{
    modelValue: unknown;
    fieldType: string;
}>();
const tempValue = ref<any>();
const emit = defineEmits<{
    "update:modelValue": [value: unknown];
}>();

function emitValue(value: unknown) {
    tempValue.value = value;
}
function blur() {
    emit("update:modelValue", tempValue.value);
}
</script>

<template>
    <InputText
        v-if="fieldType === 'string' || fieldType === 'text'"
        :modelValue="modelValue as string"
        @update:modelValue="emitValue"
        @blur="blur"
        placeholder="Enter value..."
        size="small"
        class="w-full"
    />

    <InputNumber
        v-else-if="fieldType === 'int' || fieldType === 'float'"
        :modelValue="modelValue as number | null"
        @update:modelValue="emitValue"
        @blur="blur"
        :minFractionDigits="fieldType === 'float' ? undefined : 0"
        :maxFractionDigits="fieldType === 'float' ? 10 : 0"
        placeholder="Enter number..."
        size="small"
        class="w-full"
    />

    <InputText
        v-else-if="fieldType === 'uuid' || fieldType === 'relationship'"
        :modelValue="modelValue as string"
        @update:modelValue="emitValue"
        @blur="blur"
        placeholder="Enter UUID..."
        size="small"
        class="w-full font-mono text-xs"
    />

    <DatePicker
        v-else-if="fieldType === 'datetime'"
        :modelValue="modelValue as Date | null"
        @update:modelValue="emitValue"
        @blur="blur"
        showTime
        hourFormat="24"
        placeholder="Select date/time..."
        size="small"
        class="w-full"
    />

    <InputText
        v-else
        :modelValue="modelValue as string"
        @update:modelValue="emitValue"
        @blur="blur"
        placeholder="Enter value..."
        size="small"
        class="w-full"
    />
</template>
