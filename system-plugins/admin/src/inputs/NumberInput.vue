<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
    field?: any;
    invalid?: boolean | string;
    readonly?: boolean;
}>();

const value = defineModel<number>({
    set(v: number | string) {
        if (v == null || v == "") return null;
        const n = typeof v == "number" ? v : Number(v);
        return Number.isNaN(n) ? null : n;
    },
});

const variant = computed(() => {
    const ic = props.field?.input_component;
    if (ic && ic !== "number") return ic;
    return props.field?.type == "float" ? "decimal" : "number";
});

const inputProps = computed(() => {
    switch (variant.value) {
        case "currency":
            return {
                mode: "currency",
                currency: "USD",
                locale: "en-US",
                minFractionDigits: 2,
                maxFractionDigits: 2,
            };
        case "decimal":
            return { minFractionDigits: 0, maxFractionDigits: 10 };
        case "percent":
            return { suffix: " %", minFractionDigits: 0, maxFractionDigits: 2 };
        default:
            return { minFractionDigits: 0, maxFractionDigits: 0 };
    }
});
</script>

<template>
    <InputNumber
        v-bind="inputProps"
        v-model="value"
        :placeholder="field?.default_value || undefined"
        :invalid="!!invalid"
        :readonly="readonly"
        fluid
        class="text-sm"
        :min="field.options?.min"
        :max="field.options?.max"
        :max-fraction-digits="field.options?.fraction_digits"
    />
</template>
