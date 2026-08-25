<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { Select } from "primevue";

const props = defineProps<{
    field?: any;
    visible: boolean;
}>();

const emit = defineEmits<{
    "update:visible": [value: boolean];
    select: [value: any];
}>();

interface MediaFile {
    id: string;
    filename: string;
    size_bytes: number;
    mime_type: string;
    created_at: string;
}

interface FileFolder {
    id: string;
    name: string;
    parent_id: string | null;
}

const { client } = useAlcedoClient();

const files = ref<MediaFile[]>([]),
    loading = ref(false),
    search = ref(""),
    typeFilter = ref<string>(""),
    selected = ref<any>(props.field.options.multiple ? [] : null),
    folders = ref<FileFolder[]>([]),
    currentFolderId = ref<string | null>(null),
    folderPath = ref<FileFolder[]>([]);

const fileTypes = ["image", "application", "text", "video", "audio"];

function close() {
    emit("update:visible", false);
}

async function fetchFiles() {
    loading.value = true;
    try {
        const params: {
            limit: number;
            mime_type?: string;
            folder_id?: string;
        } = { limit: 100 };
        if (typeFilter.value) params.mime_type = typeFilter.value + "/";
        if (currentFolderId.value === null) {
            params.folder_id = "";
        } else {
            params.folder_id = currentFolderId.value;
        }
        const data = (await client.files.list(params)) as { data: MediaFile[] };
        files.value = data.data || [];
    } catch {
        files.value = [];
    } finally {
        loading.value = false;
    }
}

function toggleSelect(file: MediaFile) {
    if (options.value.multiple) {
        const arr = selected.value as any[];
        const idx = arr.findIndex((f) => f == file.id);
        if (idx >= 0) arr.splice(idx, 1);
        else arr.push(file);
    } else {
        selected.value = file;
    }
}

function isSelected(id: string): boolean {
    if (options.value.multiple)
        return (selected.value as string[]).includes(id);
    return selected.value === id;
}

async function fetchFolders(parentId?: string) {
    try {
        const data = (await client.files.folders.list(parentId)) as {
            data: FileFolder[];
        };
        return data.data || [];
    } catch {
        return [];
    }
}

async function loadCurrentFolders() {
    folders.value = await fetchFolders(currentFolderId.value || undefined);
}

async function buildFolderPath(folderId: string | null): Promise<FileFolder[]> {
    const path: FileFolder[] = [];
    let current = folderId;
    while (current) {
        try {
            const folder = (await client.files.folders.get(
                current,
            )) as FileFolder;
            path.unshift(folder);
            current = folder.parent_id;
        } catch {
            break;
        }
    }
    return path;
}

function navigateToFolder(folder: FileFolder | null) {
    currentFolderId.value = folder ? folder.id : null;
    loadCurrentFolders();
    fetchFiles();
    buildFolderPath(currentFolderId.value).then((p) => (folderPath.value = p));
}

function navigateToRoot() {
    currentFolderId.value = null;
    loadCurrentFolders();
    fetchFiles();
    folderPath.value = [];
}

function confirm() {
    console.log(selected.value);
    emit("select", selected.value);
    close();
}

watch(
    () => props.visible,
    (v) => {
        if (v) {
            selected.value = options.value.multiple ? [] : null;
            currentFolderId.value = null;
            folderPath.value = [];

            if (options.value.folder_id) {
                console.log("Loading it!");
                navigateToFolder({ id: options.value.folder_id } as any);
            } else {
                loadCurrentFolders();
                fetchFiles();
            }
        }
    },
);

const options = computed(() => {
    const { options } = props.field;

    // TODO: decline files that are not the allowed mime types/folder ID
    return {
        allowed_mime_types: options.allowed_mime_types || [],
        folder_id: options.folder_id || null,
        max_file_size: options.max_file_size || [],
        multiple: options.multiple || false,
    };
});
</script>

<template>
    <Dialog
        :visible="visible"
        @update:visible="emit('update:visible', $event)"
        header="Media Library"
        :style="{ width: '50rem' }"
        :modal="true"
    >
        <div>
            <!-- Folder breadcrumb -->
            <div class="flex items-center gap-1 mb-3 text-sm">
                <button
                    class="text-gray-500 hover:text-primary transition-colors cursor-pointer flex items-center gap-1"
                    @click="navigateToRoot()"
                >
                    <i class="pi pi-home text-xs"></i>
                    <span>All Files</span>
                </button>
                <template v-for="(f, i) in folderPath" :key="f.id">
                    <i class="pi pi-chevron-right text-xs text-gray-400"></i>
                    <button
                        class="hover:text-primary transition-colors cursor-pointer"
                        :class="
                            i === folderPath.length - 1
                                ? 'text-gray-900 font-medium'
                                : 'text-gray-500'
                        "
                        @click="navigateToFolder(f)"
                    >
                        {{ f.name }}
                    </button>
                </template>
            </div>

            <!-- Subfolder buttons -->
            <div v-if="folders.length > 0" class="flex flex-wrap gap-2 mb-3">
                <button
                    v-for="folder in folders"
                    :key="folder.id"
                    class="flex items-center gap-1 px-3 py-1.5 text-sm rounded-lg border border-gray-200 hover:border-gray-400 hover:bg-gray-50 transition-colors cursor-pointer"
                    @click="navigateToFolder(folder)"
                >
                    <i class="pi pi-folder text-gray-500"></i>
                    <span>{{ folder.name }}</span>
                </button>
            </div>

            <!-- Search & filter -->
            <div class="flex gap-2 mb-4">
                <InputText
                    v-model="search"
                    placeholder="Search files..."
                    class="flex-1"
                />
                <Select
                    v-model="typeFilter"
                    :options="fileTypes"
                    placeholder="All types"
                    class="w-40"
                    :showClear="true"
                />
            </div>

            <!-- Files grid (same as before) -->
            <div v-if="loading" class="text-center py-8 text-gray-400">
                Loading...
            </div>

            <div
                v-else-if="files.length === 0"
                class="text-center py-8 text-gray-400"
            >
                No files found
            </div>

            <div v-else class="grid grid-cols-4 gap-3 max-h-96 overflow-y-auto">
                <div
                    v-for="file in files"
                    :key="file.id"
                    class="border rounded-lg p-2 cursor-pointer transition-colors"
                    :class="
                        isSelected(file.id)
                            ? 'border-primary bg-primary/10'
                            : 'border-gray-200 hover:border-gray-400'
                    "
                    @click="toggleSelect(file)"
                >
                    <div
                        class="aspect-square flex items-center justify-center bg-gray-50 rounded mb-1 overflow-hidden"
                    >
                        <img
                            v-if="file.mime_type.startsWith('image/')"
                            :src="`/api/files/${file.id}/download`"
                            class="w-full h-full object-cover"
                        />
                        <i v-else class="pi pi-file text-2xl text-gray-400"></i>
                    </div>
                    <div class="text-xs truncate">{{ file.filename }}</div>
                </div>
            </div>
        </div>

        <template #footer>
            <Button label="Cancel" severity="secondary" @click="close" />
            <Button
                label="Select"
                :disabled="
                    options.multiple
                        ? (selected as string[]).length === 0
                        : !selected
                "
                @click="confirm"
            />
        </template>
    </Dialog>
</template>
