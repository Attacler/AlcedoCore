<script lang="ts" setup>
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import { ref } from "vue";

const { folderID } = defineProps<{ folderID: string | null }>(),
    emit = defineEmits(["fileUploaded"]);

const visible = ref(false),
    uploading = ref(false),
    fileUploadInput = ref(),
    uploadProgress = ref(0);

const { client } = useAlcedoClient(),
    toast = useToast();

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
    uploading.value = true;
    uploadProgress.value = 0;

    try {
        const upload = await client.files.upload(file, file.name, {
            folder_id: folderID || undefined,
        });
        toast.show("File uploaded successfully", "success");
        emit("fileUploaded", upload);
        visible.value = false;
    } catch (e) {
        uploading.value = false;
        toast.show("Upload failed", "error");
    }
}

function focusInput() {
    fileUploadInput.value.click();
}
</script>

<template>
    <slot :openFileupload="() => (visible = true)">
        <Button
            icon="pi pi-upload"
            label="Upload"
            severity="primary"
            size="small"
            @click="visible = true"
        />
    </slot>
    <Dialog v-model:visible="visible" header="Upload file" modal>
        <div
            class="mb-6 border-2 border-dashed border-gray-300 rounded-xl p-8 text-center transition-colors hover:bg-blue-200/50 hover:border-blue-500/50 cursor-pointer"
            :class="{ 'border-primary bg-primary/5': uploading }"
            @drop.prevent="onDrop"
            @dragover.prevent
            @click="focusInput"
        >
            <div v-if="!uploading" class="space-y-3">
                <i class="pi pi-cloud-upload text-4xl text-gray-400 block"></i>
                <p class="text-gray-500">
                    Drag & drop files here, or click here to browse
                    <input
                        type="file"
                        class="hidden"
                        @change="onFileSelected"
                        ref="fileUploadInput"
                    />
                </p>
            </div>
            <div v-else class="space-y-3">
                <i
                    class="pi pi-spin pi-spinner text-3xl text-primary block"
                ></i>
                <p class="text-gray-500">Uploading...</p>
                <ProgressBar :value="uploadProgress" class="max-w-md mx-auto" />
            </div>
        </div>
    </Dialog>
</template>
