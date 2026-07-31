<script setup lang="ts">
const props = defineProps({
        plugin: { type: Object, required: true },
    }),
    emit = defineEmits(["click"]);
</script>

<template>
    <div
        class="flex items-center p-3 gap-4 border-b border-gray-200 cursor-pointer hover:bg-gray-50 transition-colors"
        @click="$emit('click')"
    >
        <div class="flex-1 flex items-center gap-2">
            <span class="font-medium text-gray-900">{{ plugin.name }}</span>
            <span class="text-sm text-gray-500">v{{ plugin.version }}</span>
        </div>
        <div class="flex gap-2">
            <span
                class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
                :class="{
                    'bg-green-100 text-green-800': plugin.status === 'enabled',
                    'bg-yellow-100 text-yellow-800':
                        plugin.status === 'disabled',
                }"
                >{{ plugin.status }}</span
            >
            <span
                class="px-2 py-0.5 rounded-full text-xs font-medium capitalize w-18 text-center"
                :class="{
                    'bg-blue-100 text-blue-800':
                        plugin.plugin_type === 'system',
                    'bg-gray-100 text-gray-700': plugin.plugin_type === 'user',
                }"
                >{{ plugin.plugin_type }}</span
            >
        </div>
    </div>
</template>
