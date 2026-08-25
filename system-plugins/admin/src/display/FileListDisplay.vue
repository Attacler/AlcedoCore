<script setup lang="ts">
import { computed } from "vue";
import { formatFileSize } from "@/utils/formatters";

defineOptions({ inheritAttrs: false });

interface FileMeta {
    id: string;
    filename: string;
    size_bytes: number;
    mime_type: string;
}

const props = defineProps<{
    value: FileMeta[] | null | undefined;
}>();

function isImage(file: FileMeta) {
    return file.mime_type?.startsWith("image/") ?? false;
}

function download(file: FileMeta) {
    if (!props.value) return;
    window.open(`/api/files/${file.id}/download`, "_blank");
}

const fileCount = computed(() => props.value?.length ?? 0);
</script>

<template>
    <div v-if="value && value.length" class="file-list-display">
        <div
            class="flex items-center gap-3 p-2 border rounded-lg"
            v-for="file of value"
            :key="file.id"
        >
            <img
                v-if="isImage(file)"
                :src="`/api/files/${file.id}/download`"
                class="w-10 h-10 object-cover rounded"
            />
            <i v-else class="pi pi-file text-xl text-gray-400"></i>
            <div class="flex-1 min-w-0">
                <div class="text-sm font-medium truncate">{{ file.filename }}</div>
                <div class="text-xs text-gray-500">
                    {{ formatFileSize(file.size_bytes) }} &middot;
                    {{ file.mime_type }}
                </div>
            </div>
            <Button
                icon="pi pi-download"
                text
                severity="secondary"
                @click="download(file)"
            />
        </div>
    </div>
    <span v-else-if="fileCount === 0" class="text-sm text-gray-300">—</span>
</template>
