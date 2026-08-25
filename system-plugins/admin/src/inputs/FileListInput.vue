<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { formatFileSize } from "@/utils/formatters";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import {
    useFileUpload,
    FileConflictError,
    type UploadedFile,
} from "@/composables/useFileUpload";
import Dialog from "primevue/dialog";
import MediaPickerModal from "./MediaPickerModal.vue";

const props = withDefaults(
    defineProps<{
        modelValue?: string[];
        field?: any;
        invalid?: boolean | string;
        readonly?: boolean;
    }>(),
    {
        modelValue: () => [],
    },
);

const emit = defineEmits<{
    "update:modelValue": [value: string[]];
}>();

interface FileItem {
    id: string;
    filename: string;
    size_bytes: number;
    mime_type: string;
}

const { client } = useAlcedoClient();
const { uploading, progress, upload } = useFileUpload();

const fileInput = ref<HTMLInputElement | null>(null),
    items = ref<FileItem[]>([]),
    showPicker = ref(false),
    dragIndex = ref<number | null>(null),
    showConflict = ref(false),
    conflictFile = ref<File | null>(null),
    conflictFilename = ref(""),
    conflictRename = ref("");

const folderId = computed(() => props.field?.options?.folder_id || undefined);

function isImage(mime: string) {
    return mime.startsWith("image/");
}

function browse() {
    fileInput.value?.click();
}

function onFileSelected(e: Event) {
    const target = e.target as HTMLInputElement;
    if (target.files?.length) {
        startUpload(target.files[0]);
        target.value = "";
    }
}

function onDrop(e: DragEvent) {
    if (e.dataTransfer?.files.length) {
        startUpload(e.dataTransfer.files[0]);
    }
}

async function startUpload(file: File) {
    try {
        const result = await upload(file, { folderId: folderId.value });

        if (props.field.options.multiple) {
            items.value.push({
                id: result.id,
                filename: result.filename,
                size_bytes: result.size_bytes,
                mime_type: result.mime_type,
            });
        } else {
            items.value = [result];
        }
        emit(
            "update:modelValue",
            items.value.map((i) => i.id),
        );
    } catch (err) {
        if (err instanceof FileConflictError) {
            conflictFile.value = err.file;
            conflictFilename.value = err.filename;
            conflictRename.value = "";
            showConflict.value = true;
        }
    }
}

async function confirmOverwrite() {
    showConflict.value = false;
    const file = conflictFile.value;
    conflictFile.value = null;
    if (!file) return;
    try {
        const result = await upload(file, {
            overwrite: true,
            folderId: folderId.value,
        });
        if (props.field.options.multiple) {
            items.value.push({
                id: result.id,
                filename: result.filename,
                size_bytes: result.size_bytes,
                mime_type: result.mime_type,
            });
        } else {
            items.value = [result];
        }
        emit(
            "update:modelValue",
            items.value.map((i) => i.id),
        );
    } catch {
        // upload failed even with overwrite — keep previous value
    }
}

async function confirmRename() {
    const newName = conflictRename.value?.trim();
    if (!newName) return;
    showConflict.value = false;
    const file = conflictFile.value;
    conflictFile.value = null;
    if (!file) return;
    try {
        const result = await upload(file, {
            filename: newName,
            folderId: folderId.value,
        });
        if (props.field.options.multiple) {
            items.value.push({
                id: result.id,
                filename: result.filename,
                size_bytes: result.size_bytes,
                mime_type: result.mime_type,
            });
        } else {
            items.value = [result];
        }
        emit(
            "update:modelValue",
            items.value.map((i) => i.id),
        );
    } catch (err) {
        if (err instanceof FileConflictError) {
            conflictFile.value = err.file;
            conflictFilename.value = err.filename;
            conflictRename.value = "";
            showConflict.value = true;
        }
    }
}

function cancelOverwrite() {
    showConflict.value = false;
    conflictFile.value = null;
}

function removeFile(index: number) {
    items.value.splice(index, 1);
    emit(
        "update:modelValue",
        items.value.map((i) => i.id),
    );
}

function onPickFromLibrary(files: any | any[]) {
    const filesList = Array.isArray(files) ? files : [files];

    if (props.field.options.multiple) {
        for (const file of filesList) {
            items.value.push(file);
        }
    } else {
        items.value = filesList;
    }
    emit(
        "update:modelValue",
        items.value.map((i) => i.id),
    );
    showPicker.value = false;
}

function onDragStart(index: number) {
    dragIndex.value = index;
}

function onDragOver(e: DragEvent, index: number) {
    e.preventDefault();
    if (dragIndex.value === null || dragIndex.value === index) return;
    const item = items.value.splice(dragIndex.value, 1)[0];
    items.value.splice(index, 0, item);
    dragIndex.value = index;
}

function onDragEnd() {
    dragIndex.value = null;
    emit(
        "update:modelValue",
        items.value.map((i) => i.id),
    );
}

async function loadFileInfos() {
    for (const entry of props.modelValue) {
        // The item API may return full file metadata objects (augmented) rather
        // than plain ID strings — use those directly when present.
        if (entry && typeof entry === "object" && (entry as any).id) {
            const meta = entry as any;
            items.value.push({
                id: meta.id,
                filename: meta.filename || meta.id,
                size_bytes: meta.size_bytes || 0,
                mime_type: meta.mime_type || "application/octet-stream",
            });
            continue;
        }
        const id = String(entry);
        try {
            const info = (await client.files.get(id)) as UploadedFile;
            items.value.push({
                id: info.id,
                filename: info.filename,
                size_bytes: info.size_bytes,
                mime_type: info.mime_type,
            });
        } catch {
            items.value.push({
                id,
                filename: id,
                size_bytes: 0,
                mime_type: "application/octet-stream",
            });
        }
    }
}

onMounted(() => {
    if (props.modelValue?.length) {
        loadFileInfos();
    }
});
</script>

<template>
    <div class="file-list-input">
        <div
            class="upload-zone border-2 border-dashed border-gray-300 rounded-lg p-4 text-center cursor-pointer hover:border-primary transition-colors mb-2"
            @drop.prevent="onDrop"
            @dragover.prevent
            @click="browse"
        >
            <i class="pi pi-upload text-xl text-gray-400 mb-1 block"></i>
            <p class="text-sm text-gray-500">
                Drag & drop a file here, or
                <a class="text-primary cursor-pointer">browse</a>
            </p>
            <input
                ref="fileInput"
                type="file"
                hidden
                @change="onFileSelected"
            />
        </div>

        <div v-if="uploading" class="progress-bar mb-2">
            <ProgressBar :value="progress" />
        </div>

        <div class="file-list space-y-1">
            <div
                v-for="(item, index) in items"
                :key="item.id"
                class="flex items-center gap-3 border rounded-lg p-2"
                :draggable="field.options.multiple"
                @dragstart="
                    field.options.multiple ? onDragStart(index) : undefined
                "
                @dragover="
                    field.options.multiple
                        ? onDragOver($event, index)
                        : undefined
                "
                @dragend="field.options.multiple ? onDragEnd : undefined"
                :class="{
                    'cursor-grab': field.options.multiple,
                }"
            >
                <i
                    class="pi pi-bars text-gray-400 cursor-grab"
                    v-if="field.options.multiple"
                ></i>
                <img
                    v-if="isImage(item.mime_type)"
                    :src="`/api/files/${item.id}/download`"
                    class="w-10 h-10 object-cover rounded"
                />
                <i v-else class="pi pi-file text-xl text-gray-400"></i>
                <div class="flex-1 min-w-0">
                    <div class="text-sm truncate">
                        {{ item.filename || item.id }}
                    </div>
                    <div v-if="item.size_bytes" class="text-xs text-gray-500">
                        {{ formatFileSize(item.size_bytes) }}
                    </div>
                </div>
                <Button
                    icon="pi pi-trash"
                    severity="danger"
                    text
                    @click="removeFile(index)"
                />
            </div>
        </div>

        <div
            v-if="items.length > 0 && field.options.multiple"
            class="text-xs text-gray-400 mb-2"
        >
            {{ items.length }} file(s)
        </div>

        <Button
            label="Pick from Media Library"
            icon="pi pi-folder"
            severity="secondary"
            text
            @click="showPicker = true"
        />

        <MediaPickerModal
            v-model:visible="showPicker"
            :multiple="true"
            @select="onPickFromLibrary"
            :field="field"
        />

        <Dialog
            v-model:visible="showConflict"
            header="File already exists"
            modal
            :style="{ width: '420px' }"
        >
            <div class="space-y-3">
                <p class="text-sm text-gray-600">
                    A file named
                    <span class="font-medium">{{ conflictFilename }}</span>
                    already exists.
                </p>
                <div>
                    <label class="block text-xs font-medium text-gray-600 mb-1"
                        >Rename to</label
                    >
                    <InputText
                        v-model="conflictRename"
                        :placeholder="conflictFilename"
                        class="w-full"
                        fluid
                        @keyup.enter="confirmRename"
                    />
                    <p class="text-xs text-gray-400 mt-1">
                        Upload under a different name, or replace the existing
                        file.
                    </p>
                </div>
            </div>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    @click="cancelOverwrite"
                />
                <Button
                    label="Replace"
                    severity="danger"
                    icon="pi pi-check"
                    @click="confirmOverwrite"
                />
                <Button
                    label="Rename"
                    severity="primary"
                    icon="pi pi-pencil"
                    :disabled="!conflictRename?.trim()"
                    @click="confirmRename"
                />
            </template>
        </Dialog>
    </div>
</template>
