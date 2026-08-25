<script setup lang="ts">
import InputNumber from "primevue/inputnumber";

const props = defineProps<{
    field: any;
}>();
</script>

<template>
    <div class="space-y-3 pt-4 border-t border-gray-200">
        <h4 class="text-sm font-semibold text-gray-700">Multi-Line Options</h4>
        <div>
            <label class="block text-xs font-medium text-gray-600 mb-1"
                >Rows</label
            >
            <InputNumber
                v-model="field.options.rows"
                :min="0"
                class="w-full"
                fluid
            />
            <p class="text-xs text-gray-400 mt-1">0 = auto resize</p>
        </div>
        <div>
            <label class="block text-xs font-medium text-gray-600 mb-1"
                >Max Length</label
            >
            <InputNumber
                v-model="field.options.max_length"
                :min="0"
                class="w-full"
                fluid
            />
            <p class="text-xs text-gray-400 mt-1">0 = no limit</p>
        </div>
    </div>
</template>
