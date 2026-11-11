<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useUsersStore } from "@/stores/usersStore";
import { useAuthStore } from "@/stores/authStore";
import { useRolesStore } from "@/stores/rolesStore";
import { useAppContextStore } from "@/stores/appContext";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import SessionsPanel from "@/components/SessionsPanel.vue";
import type { Role } from "@/stores/rolesStore";
import type { User } from "@/types/user";
import { appPath } from "@/utils/appHeaders";
import Password from "primevue/password";
import Select from "primevue/select";
import MultiSelect from "primevue/multiselect";

const route = useRoute(),
    router = useRouter(),
    authStore = useAuthStore(),
    usersStore = useUsersStore(),
    rolesStore = useRolesStore(),
    appContext = useAppContextStore(),
    { client } = useAlcedoClient();

const isAppContext = computed(() => !!appContext.appSlug);

const listPath = computed(() =>
    isAppContext.value ? appPath("/settings/users") : "/users",
);

const user = ref<User | null>(null),
    loading = ref(true),
    loadError = ref<string | null>(null),
    saving = ref(false),
    saveError = ref<string | null>(null),
    showDeleteDialog = ref(false),
    deleting = ref(false);

const editValues = ref<Record<string, any>>({}),
    editPassword = ref(""),
    repeatPassword = ref(""),
    currentPassword = ref(""),
    passwordSaving = ref(false),
    passwordError = ref<string | null>(null);

const userRoles = ref<Role[]>([]),
    selectedRoleId = ref<string | null>(null),
    roleError = ref<string | null>(null);

interface AppAccessEntry {
    app_id: number;
    app_name: string;
    api_name: string;
    version: string;
    roles: string[];
}

interface AppVersionOption {
    key: string;
    label: string;
    api_name: string;
    version: string;
}

const appAccess = ref<AppAccessEntry[]>([]),
    appAccessLoading = ref(false),
    appAccessError = ref<string | null>(null),
    appVersionOptions = ref<AppVersionOption[]>([]),
    selectedAppKey = ref<string | null>(null),
    appRoles = ref<{ id: string; name: string }[]>([]),
    selectedAppRoleIds = ref<string[]>([]),
    appRolesLoading = ref(false),
    appAccessSaving = ref(false);

const selectedAppVersion = computed(
    () =>
        appVersionOptions.value.find((o) => o.key === selectedAppKey.value) ??
        null,
);

async function loadAppAccess() {
    if (!user.value || !authStore.isAdmin) return;
    appAccessLoading.value = true;
    appAccessError.value = null;
    try {
        const access = (await client.apps.getUserAccess(
            user.value.id,
        )) as AppAccessEntry[];
        appAccess.value = access ?? [];
    } catch (e: any) {
        appAccessError.value = e?.message || "Failed to load app access";
    } finally {
        appAccessLoading.value = false;
    }
}

async function loadAppOptions() {
    if (!authStore.isAdmin) return;
    try {
        const entries = (await client.apps.me()) as AppAccessEntry[];
        appVersionOptions.value = entries.map((e) => ({
            key: `${e.api_name}::${e.version}`,
            label: `${e.app_name} (${e.version})`,
            api_name: e.api_name,
            version: e.version,
        }));
    } catch (e: any) {
        appAccessError.value = e?.message || "Failed to load apps";
    }
}

async function loadAppRoles() {
    appRoles.value = [];
    selectedAppRoleIds.value = [];
    const target = selectedAppVersion.value;
    if (!target) return;
    appRolesLoading.value = true;
    appAccessError.value = null;
    try {
        const res = (await client.roles.list({
            app: target.api_name,
            version: target.version,
        })) as { data?: { id: string; name: string }[] };
        const data = res?.data ?? res ?? [];
        appRoles.value = (Array.isArray(data) ? data : []) as {
            id: string;
            name: string;
        }[];
        const existing = appAccess.value.find(
            (a) =>
                a.api_name === target.api_name && a.version === target.version,
        );
        if (existing) {
            const granted = new Set(existing.roles);
            selectedAppRoleIds.value = appRoles.value
                .filter((r) => granted.has(r.name))
                .map((r) => r.id);
        }
    } catch (e: any) {
        appAccessError.value = e?.message || "Failed to load roles";
    } finally {
        appRolesLoading.value = false;
    }
}

async function saveAppAccess() {
    const target = selectedAppVersion.value;
    if (!user.value || !target) return;
    appAccessSaving.value = true;
    appAccessError.value = null;
    try {
        await client.apps.setUserAccess(user.value.id, {
            app: target.api_name,
            version: target.version,
            role_ids: selectedAppRoleIds.value,
        });
        await loadAppAccess();
        await loadAppRoles();
    } catch (e: any) {
        appAccessError.value = e?.message || "Failed to save app access";
    } finally {
        appAccessSaving.value = false;
    }
}

async function revokeAppAccess(entry: AppAccessEntry) {
    if (!user.value) return;
    appAccessError.value = null;
    try {
        await client.apps.setUserAccess(user.value.id, {
            app: entry.api_name,
            version: entry.version,
            role_ids: [],
        });
        await loadAppAccess();
        if (
            selectedAppVersion.value?.api_name === entry.api_name &&
            selectedAppVersion.value?.version === entry.version
        ) {
            selectedAppRoleIds.value = [];
        }
    } catch (e: any) {
        appAccessError.value = e?.message || "Failed to revoke access";
    }
}

const isNew = computed(() =>
    ["UserNew", "AppUserNew"].includes(route.name as string),
);

const isSelf = computed(
    () => !!user.value && user.value.id === authStore.user?.id,
);

const availableRolesToAdd = computed(() => {
    const assignedIds = new Set(userRoles.value.map((r) => r.id));
    return rolesStore.roles.filter((r) => !assignedIds.has(r.id));
});

onMounted(async () => {
    if (isAppContext.value) {
        await rolesStore.fetchRoles();
    }

    if (isNew.value) {
        editValues.value = { display_name: "", email: "", is_admin: false };
    }
    loading.value = false;
});

watch(
    () => route.params.id,
    async (id) => {
        editPassword.value = "";
        repeatPassword.value = "";
        currentPassword.value = "";
        passwordError.value = null;
        if (id == "new" || !id) {
            user.value = null;
            loadError.value = null;
            userRoles.value = [];
            editValues.value = {
                display_name: "",
                email: "",
                is_admin: false,
            };
            return;
        }
        const fetched = await usersStore.fetchUser(id + "");
        if (fetched) {
            user.value = fetched;
            editValues.value = {
                display_name: fetched.display_name || "",
                email: fetched.email,
                is_admin: fetched.is_admin,
            };
            await loadUserRoles();
            if (authStore.isAdmin) {
                await Promise.all([loadAppAccess(), loadAppOptions()]);
            }
        } else {
            loadError.value = "User not found";
        }
    },
    {
        immediate: true,
    },
);

async function loadUserRoles() {
    if (!user.value || !isAppContext.value) return;
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
    if (isSelf.value && !currentPassword.value) {
        passwordError.value = "Current password is required";
        return;
    }
    if (repeatPassword.value != editPassword.value) {
        passwordError.value = "The new password dont match";
        return;
    }

    passwordSaving.value = true;
    try {
        await client.users.changePassword(user.value.id, {
            current_password: isSelf.value ? currentPassword.value : undefined,
            new_password: editPassword.value,
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

    const email = String(editValues.value.email || "").trim();
    if (!email || !email.includes("@")) {
        saveError.value = "A valid email is required";
        return;
    }

    if (isNew.value && (!editPassword.value || editPassword.value.length < 8)) {
        saveError.value = "Password must be at least 8 characters";
        return;
    }

    const payload: Record<string, any> = {
        email,
        display_name: editValues.value.display_name || "",
    };
    if (authStore.isAdmin) {
        payload.is_admin = !!editValues.value.is_admin;
    }

    saving.value = true;
    try {
        if (isNew.value) {
            try {
                const result = (await client.users.create({
                    ...payload,
                    password: editPassword.value,
                })) as any;

                router.push(`${listPath.value}/${result.id}`);
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
        router.push(listPath.value);
    }
}

function handleSessionsRevokedAll() {
    if (user.value && user.value.id === authStore.user?.id) {
        router.push("/login");
    }
}
</script>

<template>
    <div class="space-y-6">
        <div class="flex items-center gap-3 mb-3">
            <router-link
                :to="listPath"
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
            <!-- User profile fields -->
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
                    <div class="grid gap-4 max-w-lg">
                        <div class="flex flex-col gap-1">
                            <label class="text-sm font-medium"
                                >Display Name</label
                            >
                            <InputText
                                v-model="editValues.display_name"
                                placeholder="Display name"
                                fluid
                            />
                        </div>
                        <div class="flex flex-col gap-1">
                            <label class="text-sm font-medium">Email</label>
                            <InputText
                                v-model="editValues.email"
                                placeholder="Email"
                                type="email"
                                fluid
                            />
                        </div>
                        <div class="flex items-center justify-between">
                            <div>
                                <div class="text-sm font-medium">
                                    Administrator
                                </div>
                                <div class="text-xs text-gray-500">
                                    Full system access
                                </div>
                            </div>
                            <ToggleSwitch
                                v-model="editValues.is_admin"
                                :disabled="!authStore.isAdmin"
                            />
                        </div>
                    </div>
                    <div v-if="saveError" class="text-sm text-red-600 mt-2">
                        {{ saveError }}
                    </div>
                    <div class="flex gap-2 mt-4">
                        <Button
                            v-if="isNew || authStore.isAdmin || isSelf"
                            :label="isNew ? 'Create User' : 'Save Changes'"
                            icon="pi pi-check"
                            :loading="saving"
                            @click="handleSave"
                        />
                        <Button
                            v-if="!isNew && (authStore.isAdmin || isSelf)"
                            label="Reset"
                            severity="secondary"
                            outlined
                            @click="resetForm"
                        />
                    </div>
                </template>
            </Card>

            <!-- Roles Card (app context only) // TODO: allow adding roles while creating a new user -->
            <Card v-if="!isNew && isAppContext">
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

            <!-- App Access Card (global zone, admins only) -->
            <Card v-if="!isNew && !isAppContext && authStore.isAdmin">
                <template #title>
                    <div class="flex items-center gap-2">
                        <span class="material-symbols-outlined text-teal-500"
                            >apps</span
                        >
                        <span>App Access</span>
                    </div>
                </template>
                <template #content>
                    <div class="space-y-4">
                        <div
                            v-if="appAccessLoading"
                            class="text-sm text-gray-500"
                        >
                            Loading...
                        </div>
                        <template v-else>
                            <div
                                v-if="appAccess.length === 0"
                                class="text-sm text-gray-500"
                            >
                                No app access assigned.
                            </div>
                            <div
                                v-for="entry in appAccess"
                                :key="entry.api_name + '@' + entry.version"
                                class="flex items-center gap-2 flex-wrap"
                            >
                                <Button
                                    icon="pi pi-times"
                                    text
                                    rounded
                                    severity="danger"
                                    size="small"
                                    title="Revoke access"
                                    @click="revokeAppAccess(entry)"
                                />
                                <span class="font-medium text-gray-900">{{
                                    entry.app_name
                                }}</span>
                                <span class="text-xs text-gray-500"
                                    >({{ entry.version }})</span
                                >
                                <template v-if="entry.roles.length">
                                    <Tag
                                        v-for="role in entry.roles"
                                        :key="role"
                                        :value="role"
                                        severity="info"
                                    />
                                </template>
                                <span v-else class="text-sm text-gray-500"
                                    >No roles</span
                                >
                            </div>
                        </template>

                        <div class="pt-3 border-t border-gray-100 space-y-2">
                            <p class="text-sm font-medium text-gray-700">
                                Grant or update access
                            </p>
                            <div class="flex flex-wrap items-center gap-2">
                                <Select
                                    v-model="selectedAppKey"
                                    :options="appVersionOptions"
                                    optionLabel="label"
                                    optionValue="key"
                                    placeholder="Select app & version"
                                    class="w-64"
                                    showClear
                                    @change="loadAppRoles"
                                />
                                <MultiSelect
                                    v-model="selectedAppRoleIds"
                                    :options="appRoles"
                                    optionLabel="name"
                                    optionValue="id"
                                    placeholder="Select roles"
                                    class="w-64"
                                    display="chip"
                                    :loading="appRolesLoading"
                                    :disabled="!selectedAppKey"
                                />
                                <Button
                                    icon="pi pi-check"
                                    label="Save"
                                    size="small"
                                    :disabled="!selectedAppKey"
                                    :loading="appAccessSaving"
                                    @click="saveAppAccess"
                                />
                            </div>
                            <div
                                v-if="appAccessError"
                                class="text-sm text-red-600"
                            >
                                {{ appAccessError }}
                            </div>
                        </div>
                    </div>
                </template>
            </Card>

            <!-- Sessions Card (self or admin) -->
            <Card
                v-if="
                    !isNew &&
                    user &&
                    (authStore.isAdmin || user.id === authStore.user?.id)
                "
            >
                <template #title>
                    <div class="flex items-center gap-2">
                        <span class="material-symbols-outlined text-purple-500"
                            >devices</span
                        >
                        <span>Sessions</span>
                    </div>
                </template>
                <template #content>
                    <SessionsPanel
                        :userId="user.id"
                        @revoked-all="handleSessionsRevokedAll"
                    />
                </template>
            </Card>

            <div class="grid md:grid-cols-2 gap-4">
                <!-- Password Card -->
                <Card v-if="isNew || authStore.isAdmin || isSelf">
                    <template #title>
                        <div class="flex items-center gap-2">
                            <span
                                class="material-symbols-outlined text-orange-500"
                            >
                                lock
                            </span>
                            <span>
                                {{
                                    isNew
                                        ? "Password"
                                        : isSelf
                                          ? "Change Password"
                                          : "Reset Password"
                                }}
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
                                <div v-if="isSelf" class="flex flex-col gap-1">
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
                                <div v-if="!isNew">
                                    <Button
                                        label="Save Password"
                                        icon="pi pi-check"
                                        :loading="passwordSaving"
                                        :disabled="
                                            (isSelf && !currentPassword) ||
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
                <Card v-if="!isNew && authStore.isAdmin">
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
