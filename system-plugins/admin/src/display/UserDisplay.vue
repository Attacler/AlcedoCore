<script setup lang="ts">
defineProps<{
    value: any;
}>();

function text(value: any): string {
    if (value == null) return "";
    if (typeof value === "object") {
        return value.display_name || value.email || value.id || "";
    }
    return String(value);
}
</script>

<template>
    <span v-if="value" class="text-sm">{{ text(value) }}</span>
    <span v-else class="text-sm text-gray-300">—</span>
</template>
