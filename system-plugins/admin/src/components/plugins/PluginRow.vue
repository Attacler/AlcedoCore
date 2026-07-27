<script setup lang="ts">
import { ref } from "vue";
import { usePluginsStore } from "@/stores/plugins";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";

const props = defineProps({
        plugin: { type: Object, required: true },
    }),
    emit = defineEmits(["click", "uninstall"]);

const store = usePluginsStore(),
    toast = useToast();

const loading = ref(false);

async function toggleEnabled() {
    loading.value = true;
    try {
        if (props.plugin.status === "enabled") {
            await store.disablePlugin(props.plugin.name);
            toast.show(`Plugin "${props.plugin.name}" disabled`, "success");
        } else {
            await store.enablePlugin(props.plugin.name);
            toast.show(`Plugin "${props.plugin.name}" enabled`, "success");
        }
    } catch (e) {
        toast.show(
            `Failed to update plugin: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        loading.value = false;
    }
}
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
                class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
                :class="{
                    'bg-blue-100 text-blue-800':
                        plugin.plugin_type === 'system',
                    'bg-gray-100 text-gray-700': plugin.plugin_type === 'user',
                }"
                >{{ plugin.plugin_type }}</span
            >
        </div>
        <div class="flex gap-2" @click.stop>
            <Button
                v-if="plugin.plugin_type === 'user'"
                :icon="
                    plugin.status === 'enabled'
                        ? 'pi pi-toggle-on'
                        : 'pi pi-toggle-off'
                "
                text
                severity="secondary"
                rounded
                @click="toggleEnabled"
                :disabled="loading"
            />
            <span
                v-if="plugin.plugin_type === 'user'"
                class="text-xs text-gray-500 self-center"
            >
                {{ plugin.status === "enabled" ? "Enabled" : "Disabled" }}
            </span>
            <Button
                v-if="plugin.plugin_type === 'system'"
                :label="plugin.status === 'enabled' ? 'Disable' : 'Enable'"
                :severity="plugin.status === 'enabled' ? 'danger' : 'success'"
                size="small"
                @click="toggleEnabled"
                :disabled="loading"
            />
            <Button
                v-if="plugin.plugin_type === 'user'"
                label="Uninstall"
                severity="danger"
                size="small"
                @click="$emit('uninstall', plugin)"
                :disabled="loading"
            />
        </div>
    </div>
</template>
