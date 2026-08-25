<script setup lang="ts">
import { watch } from "vue";
import InputNumber from "primevue/inputnumber";

const props = defineProps<{
    field: any;
    collectionName: string;
}>();

watch(
    () => props.field,
    (f) => {
        if (f) f.options = f.options || {};
    },
    { immediate: true },
);
</script>

<template>
    <div class="space-y-3 pt-4 border-t border-gray-200">
        <h4 class="text-sm font-semibold text-gray-700">Number Options</h4>
        <div class="flex gap-3">
            <div class="flex-1">
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Min</label
                >
                <InputNumber v-model="field.options.min" class="w-full" fluid />
            </div>
            <div class="flex-1">
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Max</label
                >
                <InputNumber v-model="field.options.max" class="w-full" fluid />
            </div>
        </div>
        <div>
            <label class="block text-xs font-medium text-gray-600 mb-1"
                >Fraction Digits</label
            >
            <InputNumber
                v-model="field.options.fraction_digits"
                :min="0"
                :max="10"
                class="w-full"
                fluid
            />
        </div>
    </div>
</template>
