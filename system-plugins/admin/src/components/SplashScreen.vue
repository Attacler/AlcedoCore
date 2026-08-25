<script setup lang="ts">
defineProps<{
    siteName?: string;
    logoUrl?: string;
}>();
</script>

<template>
    <div
        class="fixed inset-0 z-100 flex flex-col items-center justify-center bg-gray-100"
    >
        <img
            v-if="logoUrl"
            :src="logoUrl"
            alt=""
            class="h-12 w-12 mb-4 object-contain"
        />
        <span
            v-else
            class="mb-4 text-3xl font-bold text-gray-800 select-none"
            >{{ (siteName || "A").charAt(0) }}</span
        >
        <svg
            class="animate-spin h-8 w-8 text-blue-500"
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 24 24"
        >
            <circle
                class="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                stroke-width="4"
            ></circle>
            <path
                class="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
            ></path>
        </svg>
        <p class="mt-4 text-sm text-gray-500">Loading…</p>
    </div>
</template>
