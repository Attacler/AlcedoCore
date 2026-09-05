<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useUsersStore } from "@/stores/usersStore";
import { useAuthStore } from "@/stores/authStore";
import { useRolesStore } from "@/stores/rolesStore";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import RecordForm from "@/components/RecordForm.vue";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import type { Role } from "@/stores/rolesStore";
import type { User } from "@/types/user";
type UserData = User & { $permissions?: Record<string, unknown> };
import Password from "primevue/password";
import Select from "primevue/select";

const route = useRoute(),
    router = useRouter(),
    authStore = useAuthStore(),
    usersStore = useUsersStore(),
    rolesStore = useRolesStore(),
    { client } = useAlcedoClient();

const user = ref<UserData | null>(null),
    loading = ref(true),
    loadError = ref<string | null>(null),
    saving = ref(false),
    saveError = ref<string | null>(null),
    showDeleteDialog = ref(false),
    deleting = ref(false);

const recordFormRef = ref<any>(null),
    editValues = ref<Record<string, any>>({}),
    editPassword = ref(""),
    repeatPassword = ref(""),
    currentPassword = ref(""),
    passwordSaving = ref(false),
    passwordError = ref<string | null>(null);

const userRoles = ref<Role[]>([]),
    selectedRoleId = ref<string | null>(null),
    roleError = ref<string | null>(null);

const isNew = computed(() => route.name === "UserNew");

const availableRolesToAdd = computed(() => {
    const assignedIds = new Set(userRoles.value.map((r) => r.id));
    return rolesStore.roles.filter((r) => !assignedIds.has(r.id));
});

function canEditField(fieldName: string): boolean {
    if (!user.value?.$permissions) return true;
    const fields = user.value.$permissions.fields as string[] | undefined;
    if (!fields) return true;
    return fields.includes(fieldName);
}

onMounted(async () => {
    await rolesStore.fetchRoles();

    if (!isNew.value) {
    } else {
        editValues.value = { display_name: "", email: "", is_admin: false };
    }
    loading.value = false;
});

watch(
    () => route.params.id,
    async (id) => {
        if (id == "new" || !id) return;
        const fetched = await usersStore.fetchUser(id + "");
        if (fetched) {
            user.value = fetched;
            editValues.value = {
                display_name: fetched.display_name || "",
                email: fetched.email,
                is_admin: fetched.is_admin,
            };
            await loadUserRoles();
        } else {
            loadError.value = "User not found";
        }
    },
    {
        immediate: true,
    },
);

async function loadUserRoles() {
    if (!user.value) return;
    const roles = await rolesStore.fetchUserRoles(user.value.id);
    userRoles.value = roles;
}

async function addUserRole() {
    if (!user.value || !selectedRoleId.value) return;
    roleError.value = null;
    const ok = await rolesStore.assignRole(user.value.id, selectedRoleId.value);
    if (ok) {
        await loadUserRoles();
        selectedRoleId.value = null;
    } else {
        roleError.value = "Failed to assign role";
    }
}

async function removeUserRole(roleId: string) {
    if (!user.value) return;
    roleError.value = null;
    const ok = await rolesStore.removeRole(user.value.id, roleId);
    if (ok) {
        await loadUserRoles();
    } else {
        roleError.value = "Failed to remove role";
    }
}

function resetForm() {
    if (!user.value) return;
    editValues.value = {
        display_name: user.value.display_name || "",
        email: user.value.email,
        is_admin: user.value.is_admin,
    };
    passwordError.value = null;
}

async function handleChangePassword() {
    if (!user.value) return;
    passwordError.value = null;

    if (!editPassword.value || editPassword.value.length < 8) {
        passwordError.value = "Password must be at least 8 characters";
        return;
    }
    if (!currentPassword.value) {
        passwordError.value = "Current password is required";
        return;
    }
    if (repeatPassword.value != editPassword.value) {
        passwordError.value = "The new password dont match";
        return;
    }

    passwordSaving.value = true;
    try {
        await client.request("post", `users/${user.value.id}/password`, {
            json: {
                current_password: currentPassword.value,
                new_password: editPassword.value,
            },
        });
        editPassword.value = "";
        repeatPassword.value = "";
        currentPassword.value = "";
        passwordError.value = null;
    } catch (e: any) {
        passwordError.value = e?.message || "Failed to change password";
    } finally {
        passwordSaving.value = false;
    }
}

async function handleSave() {
    saveError.value = null;

    if (recordFormRef.value && !recordFormRef.value.validate()) {
        return;
    }

    const payload: Record<string, any> = {};
    for (const key of ["display_name", "email", "is_admin"]) {
        if (
            editValues.value[key] !== undefined &&
            editValues.value[key] !== null &&
            editValues.value[key] !== ""
        ) {
            payload[key] = editValues.value[key];
        }
    }

    if (isNew.value && (!editPassword.value || editPassword.value.length < 8)) {
        saveError.value = "Password must be at least 8 characters";
        return;
    }

    saving.value = true;
    try {
        if (isNew.value) {
            try {
                const result = (await client.users.create({
                    ...payload,
                    password: editPassword.value,
                })) as any;

                router.push(`/users/${result.data.id}`);
            } catch (e: any) {
                saveError.value =
                    e.data?.detail || e?.message || "Failed to create user";
            }
        } else if (user.value) {
            const ok = await usersStore.updateUser(user.value.id, payload);
            if (ok) {
                const fetched = await usersStore.fetchUser(user.value.id);
                if (fetched) user.value = fetched;
            } else {
                saveError.value = "Failed to save changes";
            }
        }
    } finally {
        saving.value = false;
    }
}

async function handleDelete() {
    if (!user.value) return;
    deleting.value = true;
    const ok = await usersStore.deleteUser(user.value.id);
    deleting.value = false;
    showDeleteDialog.value = false;
    if (ok) {
        router.push("/users");
    }
}
</script>

<template>
    <div class="space-y-6">
        <div class="flex items-center gap-3 mb-3">
            <router-link
                to="/users"
                class="material-symbols-outlined text-gray-400 hover:text-gray-600 transition-colors"
            >
                arrow_back
            </router-link>
            <h1 class="text-2xl font-semibold text-gray-900">
                {{
                    isNew
                        ? "New User"
                        : user?.display_name || user?.email || "User"
                }}
            </h1>
        </div>

        <div v-if="loading" class="text-center py-12 text-gray-500">
            Loading...
        </div>
        <div
            v-else-if="loadError"
            class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4"
        >
            {{ loadError }}
        </div>

        <template v-else-if="user || isNew">
            <!-- RecordForm — layout-driven user fields -->
            <Card>
                <template #title>
                    <div class="flex items-center gap-2">
                        <span class="material-symbols-outlined text-blue-500"
                            >person</span
                        >
                        <span>User Information</span>
                    </div>
                </template>
                <template #content>
                    <RecordForm
                        ref="recordFormRef"
                        collection-name="users"
                        v-model="editValues"
                        :field-readonly="
                            (field: any) => !canEditField(field.name)
                        "
                    />
                    <div v-if="saveError" class="text-sm text-red-600 mt-2">
                        {{ saveError }}
                    </div>
                    <div class="flex gap-2 mt-4">
                        <Button
                            v-if="
                                isNew
                                    ? authStore.scopes.includes('users.all')
                                    : user?.$permissions?.update
                            "
                            :label="isNew ? 'Create User' : 'Save Changes'"
                            icon="pi pi-check"
                            :loading="saving"
                            @click="handleSave"
                        />
                        <Button
                            v-if="!isNew && user?.$permissions?.update"
                            label="Reset"
                            severity="secondary"
                            outlined
                            @click="resetForm"
                        />
                    </div>
                </template>
            </Card>

            <!-- Roles Card (only for existing users) // TODO: allow adding roles while creating a new user -->
            <Card v-if="!isNew">
                <template #title>
                    <div class="flex items-center gap-2">
                        <span class="material-symbols-outlined text-purple-500"
                            >security</span
                        >
                        <span>Assigned Roles</span>
                    </div>
                </template>
                <template #content>
                    <div class="space-y-3">
                        <div
                            v-if="userRoles.length === 0"
                            class="text-sm text-gray-500"
                        >
                            No roles assigned.
                        </div>
                        <div
                            v-for="role in userRoles"
                            :key="role.id"
                            class="flex items-center gap-2"
                        >
                            <Button
                                icon="pi pi-times"
                                text
                                rounded
                                severity="danger"
                                size="small"
                                @click="removeUserRole(role.id)"
                            />
                            <Tag
                                :value="role.name"
                                :severity="role.is_system ? 'info' : 'warn'"
                            />
                            <span class="text-xs text-gray-400 flex-1">{{
                                role.description || ""
                            }}</span>
                        </div>
                        <div
                            class="flex items-center gap-2 pt-2 border-t border-gray-100"
                        >
                            <Select
                                v-model="selectedRoleId"
                                :options="availableRolesToAdd"
                                optionLabel="name"
                                optionValue="id"
                                placeholder="Add a role..."
                                class="w-64"
                                showClear
                            />
                            <Button
                                icon="pi pi-plus"
                                label="Add"
                                :disabled="!selectedRoleId"
                                size="small"
                                @click="addUserRole"
                            />
                        </div>
                        <div v-if="roleError" class="text-sm text-red-600">
                            {{ roleError }}
                        </div>
                    </div>
                </template>
            </Card>

            <div class="grid md:grid-cols-2 gap-4">
                <!-- Password Card -->
                <Card v-if="isNew || user?.$permissions?.update">
                    <template #title>
                        <div class="flex items-center gap-2">
                            <span
                                class="material-symbols-outlined text-orange-500"
                            >
                                lock
                            </span>
                            <span>
                                {{ isNew ? "Password" : "Change Password" }}
                            </span>
                        </div>
                    </template>
                    <template #content>
                        <template v-if="isNew">
                            <div class="max-w-lg space-y-4">
                                <div class="flex flex-col gap-1">
                                    <label class="text-sm font-medium">
                                        Password
                                    </label>
                                    <Password
                                        v-model="editPassword"
                                        placeholder="Min 8 characters"
                                        :feedback="false"
                                        toggleMask
                                        fluid
                                    />
                                </div>
                            </div>
                        </template>
                        <template v-else>
                            <div class="max-w-lg space-y-4">
                                <div class="flex flex-col gap-1">
                                    <label class="text-sm font-medium"
                                        >Current Password</label
                                    >
                                    <Password
                                        v-model="currentPassword"
                                        placeholder="Enter current password"
                                        :feedback="false"
                                        toggleMask
                                        fluid
                                    />
                                </div>
                                <div class="flex flex-col gap-1">
                                    <label class="text-sm font-medium"
                                        >New Password</label
                                    >
                                    <Password
                                        v-model="editPassword"
                                        placeholder="Min 8 characters"
                                        :feedback="false"
                                        toggleMask
                                        fluid
                                    />
                                </div>
                                <div class="flex flex-col gap-1">
                                    <label class="text-sm font-medium"
                                        >Repeat new Password</label
                                    >
                                    <Password
                                        v-model="repeatPassword"
                                        placeholder="Min 8 characters"
                                        :feedback="false"
                                        toggleMask
                                        fluid
                                    />
                                </div>
                                <div
                                    v-if="passwordError"
                                    class="text-sm text-red-600"
                                >
                                    {{ passwordError }}
                                </div>
                                <div>
                                    <Button
                                        label="Save Password"
                                        icon="pi pi-check"
                                        :loading="passwordSaving"
                                        :disabled="
                                            !currentPassword ||
                                            !editPassword ||
                                            !repeatPassword
                                        "
                                        @click="handleChangePassword"
                                    />
                                </div>
                            </div>
                        </template>
                    </template>
                </Card>

                <!-- Danger Zone (only for existing users) -->
                <Card v-if="!isNew && user?.$permissions?.delete">
                    <template #title>
                        <div class="flex items-center gap-2 text-red-600">
                            <span class="material-symbols-outlined"
                                >warning</span
                            >
                            <span>Danger Zone</span>
                        </div>
                    </template>
                    <template #content>
                        <div class="flex items-center justify-between">
                            <div>
                                <p class="text-sm font-medium">
                                    Delete this user
                                </p>
                                <p class="text-xs text-gray-500">
                                    This action cannot be undone.
                                </p>
                            </div>
                            <Button
                                label="Delete User"
                                icon="pi pi-trash"
                                severity="danger"
                                @click="showDeleteDialog = true"
                            />
                        </div>
                    </template>
                </Card>
            </div>
        </template>

        <ConfirmDialog
            :visible="showDeleteDialog"
            header="Delete User?"
            :message="`Delete ${user?.display_name || user?.email}? This cannot be undone.`"
            :loading="deleting"
            @confirm="handleDelete"
            @cancel="showDeleteDialog = false"
            confirmLabel="Delete user"
        />
    </div>
</template>
