<script lang="ts" setup>
import { useToast } from "@/composables/useToast";
import {
    CollectionLayout,
    FieldDefinition,
    useCollectionsStore,
} from "@/stores/collections";
import { useRolesStore } from "@/stores/rolesStore";
import { Button, Drawer, Tab, Tabs, TabList, TabPanels } from "primevue";
import { computed, ref } from "vue";
import SaveChangesButon from "./saveChangesButon.vue";

const props = defineProps<{
        collectionName: string;
        collectionMeta: any;
        sections: any[];
    }>(),
    activeLayoutId = defineModel<string | null>("activeLayoutID"),
    collLayouts = defineModel<CollectionLayout[]>("collLayouts", {
        required: true,
    }),
    emit = defineEmits(["reloadLayouts", "reloadSections"]);

const fields = defineModel<(FieldDefinition & { _key: string })[]>("fields", {
    required: true,
});

const store = useCollectionsStore(),
    rolesStore = useRolesStore(),
    toast = useToast();

const layoutRoles = ref<{ role_id: string; role_name: string }[]>([]),
    selectedLayoutRoleIds = ref<string[]>([]),
    showLayoutDialog = ref(false),
    layoutDialogName = ref(""),
    showEditLayoutDialog = ref(false),
    editLayoutName = ref(""),
    editLayoutSaving = ref(false);

async function deleteCurrentLayout() {
    if (!activeLayoutId.value) return;
    if (
        !confirm(
            "Delete this layout? All sections in this layout will be removed.",
        )
    )
        return;
    try {
        await store.deleteLayout(props.collectionName, activeLayoutId.value);
        emit("reloadLayouts");
        if (activeLayoutId.value) emit("reloadSections");
        toast.show("Layout deleted", "success");
    } catch (e) {
        toast.show(
            `Failed to delete layout: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}

const activeLayout = computed(
    () => collLayouts.value.find((l) => l.id === activeLayoutId.value) ?? null,
);

async function openEditLayoutDialog() {
    if (!activeLayoutId.value) return;
    editLayoutName.value = activeLayout.value?.name ?? "";
    try {
        await rolesStore.fetchRoles();
        const assigned = await store.getLayoutRoles(
            props.collectionName,
            activeLayoutId.value,
        );
        layoutRoles.value = assigned;
        selectedLayoutRoleIds.value = assigned.map((r: any) => r.role_id);
        showEditLayoutDialog.value = true;
    } catch (e) {
        toast.show(
            `Failed to load layout: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}

async function saveEditLayout() {
    if (!activeLayoutId.value) return;
    const newName = editLayoutName.value.trim();
    if (!newName) {
        toast.show("Layout name cannot be empty", "error");
        return;
    }
    editLayoutSaving.value = true;
    try {
        if (newName !== activeLayout.value?.name) {
            await store.updateLayout(props.collectionName, activeLayoutId.value, {
                name: newName,
            });
            emit("reloadLayouts");
        }
        await store.setLayoutRoles(
            props.collectionName,
            activeLayoutId.value,
            selectedLayoutRoleIds.value,
        );
        showEditLayoutDialog.value = false;
        toast.show("Layout updated", "success");
    } catch (e) {
        toast.show(
            `Failed to save layout: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        editLayoutSaving.value = false;
    }
}

function toggleLayoutRole(roleId: string) {
    const idx = selectedLayoutRoleIds.value.indexOf(roleId);
    if (idx >= 0) {
        selectedLayoutRoleIds.value.splice(idx, 1);
    } else {
        selectedLayoutRoleIds.value.push(roleId);
    }
}

async function openCreateLayoutDialog() {
    layoutDialogName.value = "";
    showLayoutDialog.value = true;
}

async function saveLayout() {
    if (!layoutDialogName.value.trim()) return;
    try {
        await store.createLayout(
            props.collectionName,
            layoutDialogName.value.trim(),
        );
        showLayoutDialog.value = false;
        layoutDialogName.value = "";
        emit("reloadLayouts");
        activeLayoutId.value =
            collLayouts.value[collLayouts.value.length - 1]?.id!;
        if (activeLayoutId.value) emit("reloadSections");
        toast.show("Layout created", "success");
    } catch (e) {
        toast.show(
            `Failed to create layout: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}
</script>

<template>
    <Tabs
        :value="activeLayoutId"
        @update:value="(v) => (activeLayoutId = v + '')"
        size="small"
        class="grow!"
    >
        <TabList>
            <span class="text-xs font-bold pl-2 text-gray-500 my-auto"
                >Layouts</span
            >
            <Divider layout="vertical" />
            <Tab :value="layout.id" v-for="layout of collLayouts"
                >{{ layout.name }}
            </Tab>
            <div class="flex gap-1 ml-auto my-auto pr-4">
                <Button
                    icon="pi pi-plus"
                    severity="secondary"
                    size="small"
                    label="Layout"
                    @click="openCreateLayoutDialog"
                />
                <Button
                    v-if="activeLayoutId"
                    icon="pi pi-pencil"
                    severity="secondary"
                    size="small"
                    label="Edit layout"
                    @click="openEditLayoutDialog"
                />
                <SaveChangesButon
                    :collectionMeta="collectionMeta"
                    :collectionName="collectionName"
                    :activeLayoutId="activeLayoutId || ''"
                    :sections="sections"
                    v-model:fields="fields"
                />
            </div>
        </TabList>
        <TabPanels class="bg-transparent!">
            <template #default>
                <div
                    v-if="collLayouts.length == 0"
                    class="flex items-center py-10"
                >
                    <div class="m-auto text-center">
                        <p class="pb-2">No Layouts found.</p>
                        <Button
                            label="Click here to add a layout"
                            icon="pi pi-plus"
                            @click="openCreateLayoutDialog"
                        />
                    </div>
                </div>
                <slot name="collectionSections" v-else />
            </template>
        </TabPanels>
    </Tabs>

    <!-- Edit Layout Drawer (generic: rename + roles) -->
    <Drawer
        v-model:visible="showEditLayoutDialog"
        header="Edit Layout"
        :style="{ width: '450px' }"
        position="right"
    >
        <div class="space-y-5">
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Layout Name</label
                >
                <InputText
                    v-model="editLayoutName"
                    class="w-full"
                    fluid
                    @keydown.enter="saveEditLayout"
                />
            </div>
            <div>
                <div class="text-sm font-medium text-gray-700 mb-1">Roles</div>
                <p class="text-sm text-gray-500 mb-2">
                    Users with these roles will see this layout.
                </p>
                <div
                    v-if="rolesStore.roles.length === 0"
                    class="text-sm text-gray-400 text-center py-4"
                >
                    No roles found
                </div>
                <div
                    v-for="role in rolesStore.roles"
                    :key="role.id"
                    class="flex items-center gap-3 py-1"
                >
                    <Checkbox
                        :inputId="'role-' + role.id"
                        :binary="true"
                        :modelValue="selectedLayoutRoleIds.includes(role.id)"
                        @update:modelValue="toggleLayoutRole(role.id)"
                    />
                    <label
                        :for="'role-' + role.id"
                        class="text-sm cursor-pointer"
                        >{{ role.name }}</label
                    >
                </div>
            </div>
            <div
                class="flex justify-between items-center gap-2 pt-2 border-t border-gray-200"
            >
                <Button
                    v-if="collLayouts.length > 1"
                    label="Delete"
                    severity="danger"
                    icon="pi pi-trash"
                    @click="
                        showEditLayoutDialog = false;
                        deleteCurrentLayout();
                    "
                />
                <div class="flex gap-2 ml-auto">
                    <Button
                        label="Cancel"
                        severity="secondary"
                        @click="showEditLayoutDialog = false"
                    />
                    <Button
                        label="Save"
                        severity="primary"
                        :loading="editLayoutSaving"
                        @click="saveEditLayout"
                    />
                </div>
            </div>
        </div>
    </Drawer>

    <!-- Create Layout Dialog -->
    <Drawer
        v-model:visible="showLayoutDialog"
        header="New Layout"
        :style="{ width: '400px' }"
        position="right"
    >
        <div class="space-y-4">
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Layout Name</label
                >
                <InputText
                    v-model="layoutDialogName"
                    placeholder="e.g. Editor Layout"
                    class="w-full"
                    fluid
                    @keydown.enter="saveLayout"
                    autofocus
                />
            </div>
            <div class="flex justify-end gap-2 pt-2">
                <Button
                    label="Cancel"
                    severity="secondary"
                    @click="showLayoutDialog = false"
                />
                <Button label="Create" severity="primary" @click="saveLayout" />
            </div>
        </div>
    </Drawer>
</template>
