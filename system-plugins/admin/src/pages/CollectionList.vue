<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import { useCollectionsStore, type Collection } from "@/stores/collections";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";
import Dialog from "primevue/dialog";
import InputText from "primevue/inputtext";
import { formatDate } from "@/utils/formatters";
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
        router.push(`/collections/${collection.name}/edit`);
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
</script>

<template>
    <div>
        <!-- Header with Create Collection button -->
        <div class="flex justify-between items-center mb-6">
            <h1 class="text-2xl font-bold text-gray-900">Collections</h1>
            <Button
                label="Create Collection"
                severity="primary"
                @click="openCreateModal"
                icon="pi pi-plus"
            />
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
                        :to="`/collections/${collection.name}/edit`"
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
                        :to="`/collections/${collection.name}/edit`"
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
            </template>
        </Drawer>

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
