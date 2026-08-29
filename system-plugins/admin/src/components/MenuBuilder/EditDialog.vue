<script lang="ts" setup>
import { Drawer } from "primevue";
import { ref, watch } from "vue";
import { useRolesStore } from "@/stores/rolesStore";
import { useMenuStore } from "@/stores/menuStore";
import { useToast } from "@/composables/useToast";
import { watchEffect } from "vue";

const props = defineProps<{
        selectedMenuId: string;
    }>(),
    emit = defineEmits(["refreshMenus", "openMenu"]);

const showDialog = ref(false);

const store = useMenuStore(),
    rolesStore = useRolesStore(),
    toast = useToast();

const selectedMenuId = ref<string>(""),
    assignedRoles = ref<string[]>([]),
    newRoleId = ref("");

async function fetchMenuRoles() {
    if (!selectedMenuId.value) return;
    try {
        const res = await fetch(`/api/menus/${selectedMenuId.value}/roles`, {
            credentials: "include",
        });
        if (res.ok) {
            assignedRoles.value = (await res.json()).role_ids || [];
        }
    } catch {}
}

// TODO fix that we only have this component loaded in once so that this does not trigger every time that a menu gets changed
watchEffect(() => loadMenuForEditing(props.selectedMenuId));

async function saveRoles() {
    if (!selectedMenuId.value) return;
    try {
        await fetch(`/api/menus/${selectedMenuId.value}/roles`, {
            method: "PUT",
            headers: { "Content-Type": "application/json" },
            credentials: "include",
            body: JSON.stringify({ role_ids: assignedRoles.value }),
        });
    } catch {}
}

function addRole() {
    if (newRoleId.value && !assignedRoles.value.includes(newRoleId.value)) {
        assignedRoles.value.push(newRoleId.value);
        newRoleId.value = "";
        if (props.selectedMenuId != "+") saveRoles();
    }
}

function removeRole(roleId: string) {
    assignedRoles.value = assignedRoles.value.filter((r) => r !== roleId);
    if (props.selectedMenuId != "+") saveRoles();
}

async function loadMenuForEditing(id: string) {
    selectedMenuId.value = id;

    // TODO we should allow adding roles directly when we are creating a new menu item
    if (id != "+") await fetchMenuRoles();
}

async function createNewMenu() {
    try {
        const res = await fetch("/api/menus", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            credentials: "include",
            body: JSON.stringify({
                name: store.editMenuName,
                icon: store.editMenuIcon,
                role_ids: assignedRoles.value,
            }),
        }).then((e) => e.json());

        return res.data.id;
    } catch {}
}

async function save() {
    let newMenuID = "";
    if (props.selectedMenuId == "+") {
        newMenuID = await createNewMenu();
    } else {
        await store.saveEditMenu();
        await saveRoles();
    }
    emit("refreshMenus");
    toast.show("Menu saved", "success");
    showDialog.value = false;

    if (props.selectedMenuId == "+") {
        emit("openMenu", newMenuID);
    }
}

watch(
    () => showDialog.value,
    (newVal) => {
        if (newVal) {
            if (props.selectedMenuId == "+") {
                store.editMenuName = "";
                store.editMenuIcon = "menu";
                assignedRoles.value = [];
                newRoleId.value = "";
            }
        }
    },
);

async function handleDeleteMenu() {
    try {
        await fetch(`/api/menus/${props.selectedMenuId}`, {
            method: "DELETE",
            credentials: "include",
        });
        emit("refreshMenus");
        showDialog.value = false;
    } catch {}
}
</script>

<template>
    <slot :toggleDialog="() => (showDialog = true)"></slot>

    <Drawer v-model:visible="showDialog" position="right" header="Edit menu">
        <div v-if="selectedMenuId">
            <div class="grid gap-4 items-start">
                <div class="flex-1">
                    <label class="block text-sm font-medium text-gray-700 mb-1"
                        >Menu Name</label
                    >
                    <InputText
                        v-model="store.editMenuName"
                        placeholder="Menu name"
                        class="w-full"
                        fluid
                        autofocus
                    />
                </div>
                <div>
                    <IconPicker v-model="store.editMenuIcon" />
                </div>
            </div>
        </div>

        <div class="mt-4 pt-4 border-t border-gray-200">
            <label class="block text-sm font-medium text-gray-700 mb-2"
                >Assigned Roles</label
            >
            <div class="flex flex-wrap gap-2 mb-2">
                <span
                    v-for="roleId in assignedRoles"
                    :key="roleId"
                    class="inline-flex items-center gap-1 px-2.5 py-1 bg-blue-50 text-blue-700 rounded-full text-sm border border-blue-200"
                >
                    {{
                        rolesStore.getRoleName(roleId) || roleId.slice(0, 8)
                    }}
                    <button
                        @click="removeRole(roleId)"
                        class="text-blue-500 hover:text-blue-700 text-lg leading-none"
                    >
                        &times;
                    </button>
                </span>
                <span
                    v-if="assignedRoles.length === 0"
                    class="text-sm text-gray-400 italic"
                    >No roles assigned — menu won't be visible to
                    anyone</span
                >
            </div>
            <div class="flex gap-2">
                <select
                    v-model="newRoleId"
                    class="border border-gray-300 rounded-lg px-3 py-1.5 text-sm"
                >
                    <option value="">+ Add Role</option>
                    <option
                        v-for="role in rolesStore.roles"
                        :key="role.id"
                        :value="role.id"
                    >
                        {{ role.name }}
                    </option>
                </select>
                <Button
                    label="Add"
                    severity="secondary"
                    text
                    @click="addRole"
                    :disabled="!newRoleId"
                /></div
        >
        </div>

        <template #footer>
            <div class="flex place-content-between">
                <Button outlined @click="showDialog = false">Cancel</Button>
                <Button
                    outlined
                    @click="handleDeleteMenu"
                    severity="danger"
                    v-if="props.selectedMenuId != '+'"
                >
                    Delete
                </Button>
                <Button @click="save">Save</Button>
            </div>
        </template>
    </Drawer>
</template>
