<script setup lang="ts">
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

function thumbnailSrc(file: FileMeta) {
    return `/api/files/${file.id}/download`;
}

function download(file: FileMeta) {
    if (!props.value) return;
    window.open(`/api/files/${file.id}/download`, "_blank");
}
</script>

<template>
    <div
        v-if="value"
        class="file-display flex items-center gap-3 p-1 border border-gray-300 rounded-lg"
        v-for="file of value"
    >
        <img
            v-if="isImage(file)"
            :src="thumbnailSrc(file)"
            class="w-10 h-10 object-cover rounded"
        />
        <i v-else class="pi pi-file text-2xl text-gray-400"></i>
        <div class="flex-1 min-w-0">
            <div class="text-sm font-medium truncate">{{ file.filename }}</div>
            <div class="text-xs text-gray-500">
                {{ formatFileSize(file.size_bytes) }}
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
</template>
