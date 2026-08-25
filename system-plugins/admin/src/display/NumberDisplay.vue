<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
    value: number | null | undefined;
    field?: any;
}>();

const variant = computed(() => {
    const dc = props.field?.display_component;
    return ["currency", "decimal", "percent"].includes(dc) ? dc : "number";
});

const formattedValue = computed(() => {
    if (props.value === null || props.value === undefined) return "—";
    switch (variant.value) {
        case "currency":
            return props.value.toLocaleString(undefined, {
                style: "currency",
                currency: "USD",
            });
        case "decimal":
            return props.value.toLocaleString(undefined, {
                minimumFractionDigits: 2,
                maximumFractionDigits: 10,
            });
        case "percent":
            return `${(props.value * 100).toLocaleString(undefined, {
                maximumFractionDigits: 2,
            })}%`;
        default:
            return props.value.toLocaleString();
    }
});
</script>

<template>
    <span class="text-sm text-gray-700 tabular-nums">{{ formattedValue }}</span>
</template>
