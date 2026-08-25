<script setup lang="ts">
import { ref, watch, nextTick, onUnmounted } from "vue";
import { useSavedViewsStore } from "@/stores/savedViews";
import Select from "primevue/select";
import Dialog from "primevue/dialog";
import Button from "primevue/button";
import InputText from "primevue/inputtext";
import ConfirmDialog from "@/components/ConfirmDialog.vue";

const props = defineProps<{
    collectionName: string;
    currentConfig?: Record<string, any>;
}>();

const emit = defineEmits<{
    "view-changed": [viewId: string | null];
    save: [config: Record<string, unknown>];
}>();

const store = useSavedViewsStore();

const showActions = ref(false),
    showRenameDialog = ref(false),
    showDeleteConfirm = ref(false),
    showSaveNewDialog = ref(false),
    renameValue = ref(""),
    renameInput = ref<any>(null),
    newViewName = ref("");

const isDirty = ref(false);

// Close actions popover on outside click
let outsideClickHandler: ((e: MouseEvent) => void) | null = null;

watch(showActions, (val) => {
    if (val) {
        outsideClickHandler = (e: MouseEvent) => {
            const target = e.target as HTMLElement;
            if (!target.closest(".view-selector-container")) {
                showActions.value = false;
            }
        };
        nextTick(() =>
            document.addEventListener("click", outsideClickHandler!),
        );
    } else {
        if (outsideClickHandler) {
            document.removeEventListener("click", outsideClickHandler);
            outsideClickHandler = null;
        }
    }
});

onUnmounted(() => {
    if (outsideClickHandler) {
        document.removeEventListener("click", outsideClickHandler);
    }
});

watch(
    () => props.currentConfig,
    () => {
        if (!store.activeView) {
            isDirty.value = false;
            return;
        }
        const saved = store.activeView.config;
        const current = props.currentConfig || {};

        const sortChanged =
            (current.sort?.field || "") !== (saved.sort?.field || "") ||
            (current.sort?.order || "asc") !== (saved.sort?.order || "asc");

        const savedFilters = saved.filters || {};
        const currentFilters = current.filters || {};
        const allKeys = new Set([
            ...Object.keys(savedFilters),
            ...Object.keys(currentFilters),
        ]);
        let filtersChanged = false;
        for (const key of allKeys) {
            if (
                (savedFilters as Record<string, string>)[key] !==
                (currentFilters as Record<string, string>)[key]
            ) {
                filtersChanged = true;
                break;
            }
        }

        const renderModeChanged =
            (current.render_mode || "") !== (saved.render_mode || "");
        const viewSpecificChanged =
            JSON.stringify(current.view_specific || null) !==
            JSON.stringify(saved.view_specific || null);
        const filterConditionChanged =
            JSON.stringify(current.filterCondition || null) !==
            JSON.stringify(saved.filterCondition || null);

        isDirty.value =
            sortChanged ||
            filtersChanged ||
            renderModeChanged ||
            viewSpecificChanged ||
            filterConditionChanged;
    },
    { deep: true },
);

function onViewChange(value: any) {
    const viewId = value as string | null;
    store.setActiveView(viewId);
    showActions.value = false;
    emit("view-changed", viewId);
}

function startRename() {
    if (!store.activeView) return;
    renameValue.value = store.activeView.name;
    showRenameDialog.value = true;
    showActions.value = false;
    nextTick(() => {
        renameInput.value?.$el?.focus();
        renameInput.value?.$el?.select();
    });
}

async function confirmRename() {
    if (!store.activeView || !renameValue.value.trim()) return;
    try {
        await store.updateView(props.collectionName, store.activeView.id, {
            name: renameValue.value.trim(),
        });
        showRenameDialog.value = false;
    } catch (e) {
        console.error("Failed to rename view:", e);
    }
}

async function duplicateView() {
    if (!store.activeView) return;
    showActions.value = false;
    try {
        const newView = await store.duplicateView(
            props.collectionName,
            store.activeView.id,
        );
        store.setActiveView(newView.id);
        emit("view-changed", newView.id);
    } catch (e) {
        console.error("Failed to duplicate view:", e);
    }
}

async function makeDefault() {
    if (!store.activeView) return;
    showActions.value = false;
    try {
        await store.setDefaultView(props.collectionName, store.activeView.id);
    } catch (e) {
        console.error("Failed to set default view:", e);
    }
}

async function confirmDelete() {
    if (!store.activeView) return;
    const viewId = store.activeView.id;
    showDeleteConfirm.value = false;
    showActions.value = false;
    try {
        await store.deleteView(props.collectionName, viewId);
        emit("view-changed", null);
    } catch (e) {
        console.error("Failed to delete view:", e);
    }
}

async function saveNewView() {
    if (!newViewName.value.trim()) return;
    const config = buildConfig();
    try {
        const newView = await store.createView(props.collectionName, {
            name: newViewName.value.trim(),
            config,
        });
        store.setActiveView(newView.id);
        isDirty.value = false;
        emit("view-changed", newView.id);
        showSaveNewDialog.value = false;
    } catch (e) {
        console.error("Failed to create view:", e);
    }
}

async function saveCurrentView() {
    if (!store.activeViewId || !store.activeView) return;
    const config = buildConfig();
    try {
        await store.updateView(props.collectionName, store.activeViewId, {
            config,
        });
        isDirty.value = false;
        emit("save", config);
    } catch (e) {
        console.error("Failed to save view:", e);
    }
}

function buildConfig(): Record<string, unknown> {
    const c = props.currentConfig || {};
    return {
        ...(c.filters && Object.keys(c.filters).length > 0
            ? { filters: c.filters }
            : {}),
        ...(c.sort?.field ? { sort: c.sort } : {}),
        ...(c.render_mode ? { render_mode: c.render_mode } : {}),
        ...(c.filterCondition ? { filterCondition: c.filterCondition } : {}),
        ...(c.view_specific && Object.keys(c.view_specific).length > 0
            ? { view_specific: c.view_specific }
            : {}),
    };
}
</script>

<template>
    <div class="flex items-center gap-2">
        <!-- View selector dropdown -->
        <div
            class="view-selector-container flex items-center gap-1 w-45 sm:w-[320px]"
        >
            <Select
                v-if="store.views.length > 0"
                v-model="store.activeViewId"
                :options="store.views"
                option-label="name"
                option-value="id"
                class="flex-1 min-w-0"
                placeholder="Select a view..."
                @change="onViewChange($event.value)"
            >
                <template #option="slotProps">
                    {{ slotProps.option.name
                    }}{{ slotProps.option.is_default ? " ★" : "" }}
                </template>
            </Select>

            <!-- No views state -->
            <div
                v-else-if="!store.loading"
                class="text-sm text-gray-400 italic px-2 py-2 flex-1"
            >
                No saved views
            </div>

            <!-- Actions popover for active view -->
            <div v-if="store.activeView" class="relative shrink-0">
                <Button
                    icon="pi pi-ellipsis-v"
                    text
                    severity="secondary"
                    title="View actions"
                    @click.stop="showActions = !showActions"
                />

                <!-- Actions popover -->
                <div
                    v-if="showActions"
                    class="absolute right-0 mt-1 w-44 bg-white rounded-md shadow-lg border border-gray-200 z-50 py-1"
                    @click.away="showActions = false"
                >
                    <Button
                        class="w-full text-left"
                        severity="secondary"
                        text
                        @click="startRename"
                        >Rename</Button
                    >
                    <Button
                        class="w-full text-left"
                        severity="secondary"
                        text
                        @click="duplicateView"
                        >Duplicate</Button
                    >
                    <Button
                        v-if="store.activeView && !store.activeView.is_default"
                        class="w-full text-left"
                        severity="secondary"
                        text
                        @click="makeDefault"
                        >Set as Default</Button
                    >
                    <hr class="my-1 border-gray-200" />
                    <Button
                        class="w-full text-left"
                        severity="danger"
                        text
                        @click="showDeleteConfirm = true"
                        >Delete</Button
                    >
                </div>
            </div>
        </div>

        <!-- Save button -->
        <Button
            icon="pi pi-save"
            label="Save"
            severity="secondary"
            outlined
            size="small"
            class="mobile-icon-only"
            @click="saveCurrentView"
        />

        <Button
            icon="pi pi-plus"
            label="Save new view"
            severity="secondary"
            outlined
            size="small"
            class="mobile-icon-only"
            @click="
                newViewName = 'New View';
                showSaveNewDialog = true;
            "
        />

        <Dialog
            v-model:visible="showRenameDialog"
            header="Rename View"
            :modal="true"
        >
            <div class="flex flex-col gap-4">
                <InputText
                    ref="renameInput"
                    v-model="renameValue"
                    placeholder="View name..."
                    class="w-full"
                    fluid
                    @keyup.enter="confirmRename"
                />
            </div>
            <template #footer>
                <div class="flex gap-2 justify-end">
                    <Button
                        label="Cancel"
                        severity="secondary"
                        outlined
                        @click="showRenameDialog = false"
                    />
                    <Button
                        label="Rename"
                        severity="primary"
                        @click="confirmRename"
                    />
                </div>
            </template>
        </Dialog>

        <Dialog
            v-model:visible="showSaveNewDialog"
            header="Save New View"
            :modal="true"
        >
            <div class="flex flex-col gap-4">
                <InputText
                    ref="newViewInput"
                    v-model="newViewName"
                    placeholder="View name..."
                    class="w-full"
                    fluid
                    @keyup.enter="saveNewView"
                />
            </div>
            <template #footer>
                <div class="flex gap-2 justify-end">
                    <Button
                        label="Cancel"
                        severity="secondary"
                        outlined
                        @click="showSaveNewDialog = false"
                    />
                    <Button
                        label="Save"
                        severity="primary"
                        @click="saveNewView"
                    />
                </div>
            </template>
        </Dialog>

        <ConfirmDialog
            :visible="showDeleteConfirm"
            header="Delete View"
            :message="`Are you sure you want to delete ${store.activeView?.name}? This cannot be undone.`"
            @confirm="confirmDelete"
            @cancel="showDeleteConfirm = false"
            confirmLabel="Delete view"
        />
    </div>
</template>

<style scoped>
@media (max-width: 639px) {
    :deep(.mobile-icon-only .p-button-label) {
        display: none;
    }
}
</style>
