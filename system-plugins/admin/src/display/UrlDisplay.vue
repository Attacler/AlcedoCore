<script setup lang="ts">
const props = defineProps<{
    value: string | null | undefined;
}>();
</script>

<template>
    <a
        v-if="value"
        :href="value"
        target="_blank"
        rel="noopener noreferrer"
        class="text-blue-500 hover:text-blue-700 hover:underline font-medium text-sm"
    >
        {{ value }}
    </a>
    <span v-else class="text-sm text-gray-300">—</span>
</template>
