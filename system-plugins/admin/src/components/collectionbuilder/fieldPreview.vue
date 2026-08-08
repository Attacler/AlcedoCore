<script setup lang="ts">
import { toRefs } from "vue";

const rawProps = defineProps<{
    field: any;
    dropBeforeKey: string | null;
    isDragging: boolean;
}>();
const props = toRefs(rawProps);

const emit = defineEmits([
    "onDragOverField",
    "onDragLeaveField",
    "onFieldDrop",
    "onFieldDragStart",
    "openFieldEditor",
]);
</script>

<template>
    <div
        v-if="field._isGap"
        class="w-full h-8 rounded transition-colors"
        :class="{
            'animate-pulse bg-blue-500/20':
                props.dropBeforeKey.value == field._key,
            'border-2 border-blue-400 bg-blue-100/30 border-dashed hover:border-blue-300 hover:bg-blue-50 cursor-pointer':
                props.isDragging.value,
            hidden: !props.isDragging.value,
        }"
        @dragover.prevent="emit('onDragOverField', field._key)"
        @dragleave="emit('onDragLeaveField', field._key)"
        @drop.stop="emit('onFieldDrop', $event, field._key)"
    ></div>
    <div
        v-else
        draggable="true"
        class="group relative border rounded-lg px-3 py-0.5 transition-all cursor-grab active:cursor-grabbing border-gray-200 hover:border-blue-300 hover:shadow-sm"
        @dragstart="emit('onFieldDragStart', $event, field._key)"
    >
        <div class="flex items-center justify-between">
            <div class="flex items-center gap-1.5 min-w-0">
                <span
                    v-if="!field.is_system"
                    class="text-gray-300 group-hover:text-gray-400 text-xs cursor-grab select-none"
                    >&#9776;</span
                >
                <span
                    v-else
                    class="material-symbols-outlined text-gray-300 text-sm"
                    >lock</span
                >
                <label
                    class="block text-xs font-medium truncate"
                    :class="field.is_system ? 'text-gray-400' : 'text-gray-600'"
                    ><FieldNameLabel :field="field" /><span
                        v-if="field.required"
                        class="text-red-400 ml-0.5"
                        >*</span
                    ></label
                >
            </div>
            <div class="flex items-center gap-0.5" v-if="!field.is_system">
                <Button
                    icon="pi pi-cog"
                    text
                    severity="secondary"
                    size="small"
                    @click="emit('openFieldEditor', field)"
                    title="Settings"
                    class="opacity-0 group-hover:opacity-100"
                />
            </div>
        </div>
    </div>
</template>
