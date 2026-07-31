<script lang="ts" setup>
import { PluginStore } from "@/stores/plugins";
import { useRoute } from "vue-router";

const props = defineProps<{ plugin: PluginStore }>();

const route = useRoute();
</script>

<template>
    <div v-if="plugin?.endpoints && Object.keys(plugin.endpoints).length > 0">
        <div class="space-y-2">
            <div
                v-for="(endpoint, index) in Object.values(
                    plugin.endpoints ?? {},
                )"
                :key="index"
                class="flex items-center gap-2 sm:gap-4 p-3 border border-gray-200 rounded-lg flex-wrap"
            >
                <span
                    class="px-2 py-1 rounded text-xs font-medium flex-shrink-0"
                    :class="{
                        'bg-blue-100 text-blue-800': endpoint.method === 'GET',
                        'bg-green-100 text-green-800':
                            endpoint.method === 'POST',
                        'bg-yellow-100 text-yellow-800':
                            endpoint.method === 'PUT',
                        'bg-red-100 text-red-800': endpoint.method === 'DELETE',
                        'bg-gray-100 text-gray-700': ![
                            'GET',
                            'POST',
                            'PUT',
                            'DELETE',
                        ].includes(endpoint.method),
                    }"
                    >{{ endpoint.method }}</span
                >
                <span class="font-mono text-sm break-all"
                    >/p/{{ route.params.name }}{{ endpoint.path }}</span
                >
            </div>
        </div>
    </div>
    <div v-else class="text-gray-400 italic">
        No endpoints available for this plugin
    </div>
</template>
