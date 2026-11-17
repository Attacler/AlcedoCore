<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import {
    useCollectionsStore,
    type Collection,
    type FieldType,
} from "@/stores/collections";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";
import Dialog from "primevue/dialog";
import InputText from "primevue/inputtext";
import Select from "primevue/select";
import { getInputComponentsForFieldType } from "@/inputs";
import { getDisplayComponentsForFieldType } from "@/display";
import { formatDate } from "@/utils/formatters";
import { appPath } from "@/utils/appHeaders";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import { Drawer } from "primevue";

const store = useCollectionsStore(),
    router = useRouter(),
    toast = useToast();

const sortedCollections = computed(() => {
    const cols = [...store.collections];
    cols.sort((a, b) => {
        if (a.is_system && !b.is_system) return 1;
        if (!a.is_system && b.is_system) return -1;
        return (a.display_name || a.name).localeCompare(
            b.display_name || b.name,
        );
    });
    return cols;
});

const showCreateModal = ref(false),
    showDeleteModal = ref(false),
    collectionToDelete = ref<Collection | null>(null),
    newCollectionName = ref(""),
    nameError = ref(""),
    creating = ref(false);

const NAME_REGEX = /^[a-z][a-z0-9_]*$/,
    MAX_NAME_LENGTH = 59;

onMounted(() => {
    store.fetchCollections();
});

function openCreateModal() {
    newCollectionName.value = "";
    nameError.value = "";
    creating.value = false;
    showCreateModal.value = true;
}

function closeCreateModal() {
    showCreateModal.value = false;
    newCollectionName.value = "";
    nameError.value = "";
    creating.value = false;
}

function validateName() {
    const name = newCollectionName.value;
    if (name.length === 0) {
        nameError.value = "";
        return;
    }
    if (name.length > MAX_NAME_LENGTH) {
        newCollectionName.value = name.slice(0, MAX_NAME_LENGTH);
    }
    if (!NAME_REGEX.test(name)) {
        nameError.value =
            "Name must start with a lowercase letter and contain only lowercase letters, numbers, and underscores";
    } else {
        nameError.value = "";
    }
}

async function handleCreate() {
    if (nameError.value || !newCollectionName.value.trim() || creating.value)
        return;
    creating.value = true;
    try {
        const collection = await store.createCollection({
            name: newCollectionName.value.trim(),
        });
        toast.show(
            `Collection "${collection.display_name || collection.name}" created`,
            "success",
        );
        closeCreateModal();
        router.push(appPath(`/collections/${collection.name}/edit`));
    } catch (e) {
        toast.show(
            `Failed to create collection: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        creating.value = false;
    }
}

function confirmDelete(collection: Collection) {
    collectionToDelete.value = collection;
    showDeleteModal.value = true;
}

async function handleDelete() {
    if (!collectionToDelete.value) return;
    try {
        await store.deleteCollection(collectionToDelete.value.name);
        toast.show(
            `Collection "${collectionToDelete.value.display_name || collectionToDelete.value.name}" deleted`,
            "success",
        );
        closeDeleteModal();
    } catch (e) {
        toast.show(
            `Failed to delete: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}

function closeDeleteModal() {
    showDeleteModal.value = false;
    collectionToDelete.value = null;
}

// ---------------------------------------------------------------------------
// Import collections (adopt existing tables that have no metadata)
// ---------------------------------------------------------------------------

interface UnregisteredColumnRow {
    name: string;
    field_type: string;
    required: boolean;
}

interface UnregisteredTableRow {
    name: string;
    columns: UnregisteredColumnRow[];
}

interface ImportFieldRow {
    name: string;
    field_type: FieldType;
    display_name: string;
    input_component: string;
    display_component: string;
}

const showImportModal = ref(false),
    unregisteredTables = ref<UnregisteredTableRow[]>([]),
    importLoading = ref(false),
    importError = ref(""),
    importTableSearch = ref(""),
    selectedTable = ref<UnregisteredTableRow | null>(null),
    importDisplayName = ref(""),
    importFields = ref<ImportFieldRow[]>([]),
    importFieldSearch = ref(""),
    importing = ref(false);

const filteredUnregisteredTables = computed(() => {
    const q = importTableSearch.value.trim().toLowerCase();
    return [...unregisteredTables.value]
        .sort((a, b) => a.name.localeCompare(b.name))
        .filter((t) => !q || t.name.toLowerCase().includes(q));
});

const filteredImportFields = computed(() => {
    const q = importFieldSearch.value.trim().toLowerCase();
    return importFields.value.filter(
        (f) =>
            !q ||
            f.name.toLowerCase().includes(q) ||
            f.display_name.toLowerCase().includes(q),
    );
});

async function openImportModal() {
    showImportModal.value = true;
    importLoading.value = true;
    importError.value = "";
    importTableSearch.value = "";
    selectedTable.value = null;
    unregisteredTables.value = [];
    try {
        unregisteredTables.value = await store.listUnregistered();
    } catch (e) {
        importError.value =
            e instanceof Error ? e.message : "Failed to load tables";
    } finally {
        importLoading.value = false;
    }
}

function componentOptions(
    fieldType: FieldType,
    kind: "input" | "display",
): { type: string; label: string }[] {
    const registry =
        kind === "input"
            ? getInputComponentsForFieldType(fieldType)
            : getDisplayComponentsForFieldType(fieldType);
    return [
        { type: "default", label: "Default" },
        ...registry.map((e) => ({ type: e.type, label: e.label })),
    ];
}

function selectTable(table: UnregisteredTableRow) {
    selectedTable.value = table;
    importDisplayName.value = table.name;
    importFieldSearch.value = "";
    importFields.value = [...table.columns]
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((c) => ({
            name: c.name,
            field_type: c.field_type as FieldType,
            display_name: c.name,
            input_component: "default",
            display_component: "default",
        }));
}

async function handleImport() {
    if (!selectedTable.value || importing.value) return;
    importing.value = true;
    try {
        const collection = await store.importCollection({
            table: selectedTable.value.name,
            display_name: importDisplayName.value.trim() || undefined,
            fields: importFields.value.map((f) => ({
                name: f.name,
                display_name: f.display_name,
                input_component:
                    f.input_component === "default"
                        ? undefined
                        : f.input_component,
                display_component:
                    f.display_component === "default"
                        ? undefined
                        : f.display_component,
            })),
        });
        toast.show(
            `Collection "${collection.display_name || collection.name}" imported`,
            "success",
        );
        showImportModal.value = false;
        router.push(appPath(`/collections/${collection.name}/edit`));
    } catch (e) {
        toast.show(
            `Failed to import: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        importing.value = false;
    }
}
</script>

<template>
    <div>
        <!-- Header with Create Collection button -->
        <div class="flex justify-between items-center mb-6">
            <h1 class="text-2xl font-bold text-gray-900">Collections</h1>
            <div class="flex gap-2">
                <Button
                    label="Import collections"
                    severity="secondary"
                    outlined
                    icon="pi pi-download"
                    @click="openImportModal"
                />
                <Button
                    label="Create Collection"
                    severity="primary"
                    @click="openCreateModal"
                    icon="pi pi-plus"
                />
            </div>
        </div>

        <div v-if="store.loading" class="text-center text-gray-500 py-8">
            Loading...
        </div>

        <div v-else-if="store.error" class="text-center text-red-500 py-8">
            <p class="mb-4">Failed to load: {{ store.error }}</p>
            <Button
                label="Retry"
                severity="warn"
                @click="store.fetchCollections()"
            />
        </div>

        <div
            v-else-if="sortedCollections.length > 0"
            class="bg-white rounded-lg shadow-sm"
        >
            <!-- Table Header -->
            <div
                class="hidden sm:flex items-center px-4 py-3 gap-4 border-b border-gray-200 bg-gray-50 text-xs font-medium text-gray-500 uppercase tracking-wider"
            >
                <div class="flex-1">Name</div>
                <div class="w-20 text-center">Fields</div>
                <div class="w-28 text-center">Created</div>
                <div class="w-36 text-right">Actions</div>
            </div>
            <div
                v-for="collection in sortedCollections"
                :key="collection.name"
                class="flex flex-col sm:flex-row sm:items-center p-4 gap-3 sm:gap-4 border-b border-gray-200 hover:bg-gray-50 transition-colors"
            >
                <div class="flex-1 min-w-0 flex items-center gap-2">
                    <span
                        v-if="collection.is_system"
                        class="material-symbols-outlined text-gray-400 text-lg"
                        >lock</span
                    >
                    <span
                        v-else
                        class="material-symbols-outlined text-gray-400 text-lg"
                        >folder</span
                    >
                    <router-link
                        :to="appPath(`/collections/${collection.name}/edit`)"
                        class="font-medium hover:text-blue-600 transition-colors"
                        :class="
                            collection.is_system
                                ? 'text-gray-500'
                                : 'text-gray-900'
                        "
                    >
                        {{ collection.display_name || collection.name }}
                    </router-link>
                    <Tag
                        v-if="collection.is_system"
                        value="System"
                        severity="info"
                        class="ml-1"
                    />
                </div>
                <div class="text-sm text-gray-500 sm:w-20 sm:text-center">
                    {{ collection.fields.length }}
                </div>
                <div class="text-sm text-gray-500 sm:w-28 sm:text-center">
                    {{ formatDate(collection.created_at) }}
                </div>
                <div class="flex gap-2 sm:w-36 sm:justify-end">
                    <Button
                        label="Edit"
                        severity="secondary"
                        outlined
                        as="router-link"
                        :to="appPath(`/collections/${collection.name}/edit`)"
                    />
                    <Button
                        v-if="!collection.is_system"
                        label="Delete"
                        severity="danger"
                        @click="confirmDelete(collection)"
                    />
                </div>
            </div>
        </div>

        <!-- Empty State -->
        <div v-else class="text-center py-12">
            <h3 class="text-lg font-medium text-gray-900 mb-2">
                No collections yet, create your first one
            </h3>
            <Button
                label="Create Collection"
                severity="primary"
                @click="openCreateModal"
            />
        </div>

        <Drawer
            v-model:visible="showCreateModal"
            header="Create Collection"
            :style="{ width: '450px' }"
            position="right"
        >
            <form @submit.prevent="handleCreate">
                <div class="mb-4">
                    <label
                        for="collection-name"
                        class="block text-sm font-medium text-gray-700 mb-1"
                        >Collection Name</label
                    >
                    <InputText
                        id="collection-name"
                        v-model="newCollectionName"
                        maxlength="59"
                        :invalid="!!nameError"
                        placeholder="e.g. users"
                        class="w-full"
                        fluid
                        @input="validateName"
                        autofocus
                    />
                    <div class="flex justify-between items-center mt-1">
                        <p v-if="nameError" class="text-xs text-red-500">
                            {{ nameError }}
                        </p>
                        <p v-else class="text-xs text-gray-400">&nbsp;</p>
                        <p class="text-xs text-gray-400">
                            {{ newCollectionName.length }}/59
                        </p>
                    </div>
                </div>
            </form>
            <template #footer>
                <div class="flex gap-2">
                    <Button
                        label="Cancel"
                        severity="secondary"
                        outlined
                        @click="closeCreateModal"
                    />
                    <Button
                        label="Create"
                        severity="primary"
                        :disabled="
                            !!nameError || !newCollectionName.trim() || creating
                        "
                        @click="handleCreate"
                        type="submit"
                    />
                </div>
            </template>
        </Drawer>

        <!-- Import Collections Modal -->
        <Dialog
            v-model:visible="showImportModal"
            :header="
                selectedTable
                    ? `Import ${selectedTable.name}`
                    : 'Import collections'
            "
            :style="{ width: '900px' }"
            modal
        >
            <!-- Step 1: pick a table without metadata -->
            <div v-if="!selectedTable">
                <p v-if="importLoading" class="text-gray-500 py-4 text-center">
                    Loading tables...
                </p>
                <p v-else-if="importError" class="text-red-500 py-4">
                    {{ importError }}
                </p>
                <p
                    v-else-if="unregisteredTables.length === 0"
                    class="text-gray-500 py-4 text-center"
                >
                    No tables without metadata found in this app.
                </p>
                <template v-else>
                    <IconField class="mb-3">
                        <InputIcon class="pi pi-search" />
                        <InputText
                            v-model="importTableSearch"
                            placeholder="Search tables..."
                            fluid
                        />
                    </IconField>
                    <p
                        v-if="filteredUnregisteredTables.length === 0"
                        class="text-gray-500 py-4 text-center"
                    >
                        No tables match "{{ importTableSearch }}".
                    </p>
                    <div v-else class="border border-gray-200 rounded-lg">
                        <div
                            class="flex px-4 py-2 border-b border-gray-200 bg-gray-50 text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >
                            <div class="flex-1">Table</div>
                            <div class="w-24 text-center">Columns</div>
                        </div>
                        <button
                            v-for="table in filteredUnregisteredTables"
                            :key="table.name"
                            type="button"
                            class="flex w-full px-4 py-3 border-b border-gray-100 last:border-0 hover:bg-gray-50 text-left"
                            @click="selectTable(table)"
                        >
                            <div class="flex-1 font-medium text-gray-900">
                                {{ table.name }}
                            </div>
                            <div
                                class="w-24 text-center text-sm text-gray-500"
                            >
                                {{ table.columns.length }}
                            </div>
                        </button>
                    </div>
                </template>
            </div>

            <!-- Step 2: review fields -->
            <div v-else>
                <div class="mb-4">
                    <label
                        class="block text-sm font-medium text-gray-700 mb-1"
                        >Display name</label
                    >
                    <InputText
                        v-model="importDisplayName"
                        class="w-full"
                        fluid
                    />
                </div>
                <IconField class="mb-3">
                    <InputIcon class="pi pi-search" />
                    <InputText
                        v-model="importFieldSearch"
                        placeholder="Search columns..."
                        fluid
                    />
                </IconField>
                <div class="border border-gray-200 rounded-lg">
                    <div
                        class="hidden sm:grid grid-cols-[1fr_1fr_1fr_1fr] gap-3 px-4 py-2 border-b border-gray-200 bg-gray-50 text-xs font-medium text-gray-500 uppercase tracking-wider"
                    >
                        <div>api_name</div>
                        <div>Display name</div>
                        <div>Input</div>
                        <div>Display</div>
                    </div>
                    <div
                        v-for="field in filteredImportFields"
                        :key="field.name"
                        class="grid grid-cols-1 sm:grid-cols-[1fr_1fr_1fr_1fr] gap-3 px-4 py-3 border-b border-gray-100 last:border-0 items-center"
                    >
                        <span
                            class="font-mono text-sm text-gray-700"
                            :title="field.field_type"
                            >{{ field.name }}</span
                        >
                        <InputText v-model="field.display_name" fluid />
                        <Select
                            v-model="field.input_component"
                            :options="componentOptions(field.field_type, 'input')"
                            option-label="label"
                            option-value="type"
                            class="w-full"
                            fluid
                        />
                        <Select
                            v-model="field.display_component"
                            :options="
                                componentOptions(field.field_type, 'display')
                            "
                            option-label="label"
                            option-value="type"
                            class="w-full"
                            fluid
                        />
                    </div>
                </div>
            </div>

            <template #footer>
                <div class="flex gap-2 justify-end">
                    <Button
                        v-if="selectedTable"
                        label="Back"
                        severity="secondary"
                        outlined
                        @click="selectedTable = null"
                    />
                    <Button
                        label="Cancel"
                        severity="secondary"
                        outlined
                        @click="showImportModal = false"
                    />
                    <Button
                        v-if="selectedTable"
                        label="Import collection"
                        severity="primary"
                        :loading="importing"
                        @click="handleImport"
                    />
                </div>
            </template>
        </Dialog>

        <!-- Delete Confirmation Modal -->
        <ConfirmDialog
            :visible="showDeleteModal"
            header="Delete Collection"
            :message="`Delete ${collectionToDelete?.display_name || collectionToDelete?.name}? All data in the ${collectionToDelete?.name} table will be permanently dropped. This cannot be undone.`"
            @confirm="handleDelete"
            @cancel="closeDeleteModal"
            confirmLabel="Delete collection"
        />
    </div>
</template>
