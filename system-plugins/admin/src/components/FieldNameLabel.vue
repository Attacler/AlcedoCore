<script setup lang="ts">
import { ref, computed } from "vue";
import type { FieldDefinition } from "@/stores/collections";
import { getDisplayComponentDef } from "@/display";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import { useDevMode } from "@/composables/useDevMode";
import Popover from "primevue/popover";

const props = defineProps<{
    field: FieldDefinition;
}>();

const extensionRegistry = useExtensionRegistryStore(),
    { devMode: showDevMode } = useDevMode();

const opRef = ref<InstanceType<typeof Popover>>(),
    iconRef = ref<HTMLElement>();

function togglePanel(event?: MouseEvent) {
    if (opRef.value && iconRef.value) {
        opRef.value.toggle((event || iconRef.value) as Event);
    }
}

const displayType = computed(() => {
    const dt =
        (props.field as any)._displayType ||
        props.field.display_type ||
        props.field.type;
    const entry = getDisplayComponentDef(dt);
    if (entry) return entry.label;
    const widget = extensionRegistry.getDisplayWidget(dt);
    return widget?.label || dt;
});
</script>

<template>
    <span class="inline-flex items-center gap-1">
        <span
            v-if="showDevMode"
            ref="iconRef"
            class="pi pi-code text-xs text-gray-400 hover:text-blue-500 cursor-pointer transition-colors shrink-0"
            @click.stop="togglePanel"
        />
        <span class="text-base">{{ field.display_name || field.name }}</span>
        <Popover ref="opRef" :dismissable="true">
            <div class="text-xs space-y-1.5 min-w-45">
                <div class="flex items-center gap-3">
                    <span class="text-gray-400 w-20 shrink-0"
                        >Display Name</span
                    >
                    <span class="text-gray-800 font-medium">{{
                        field.display_name || "—"
                    }}</span>
                </div>
                <div class="flex items-center gap-3">
                    <span class="text-gray-400 w-20 shrink-0">API Name</span>
                    <code
                        class="text-gray-800 text-[11px] bg-gray-100 px-1 py-0.5 rounded"
                        >{{ field.name }}</code
                    >
                </div>
                <div class="flex items-center gap-3">
                    <span class="text-gray-400 w-20 shrink-0">Type</span>
                    <span class="text-gray-800">{{ field.type }}</span>
                </div>
                <div class="flex items-center gap-3">
                    <span class="text-gray-400 w-20 shrink-0"
                        >Display Type</span
                    >
                    <span class="text-gray-800">{{ displayType }}</span>
                </div>
                <div
                    v-if="
                        field.type === 'relationship' &&
                        field.related_collection
                    "
                    class="flex items-center gap-3"
                >
                    <span class="text-gray-400 w-20 shrink-0">Relates To</span>
                    <span class="text-gray-800">{{
                        field.related_collection
                    }}</span>
                </div>
            </div>
        </Popover>
    </span>
</template>
