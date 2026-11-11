<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "@/stores/authStore";
import { appPath } from "@/utils/appHeaders";
import SessionsPanel from "@/components/SessionsPanel.vue";

const authStore = useAuthStore(),
    router = useRouter();

const backTo = computed(() =>
    router.currentRoute.value.params.appSlug ? appPath("/settings") : "/apps",
);

const displayName = computed(() => {
    return (
        authStore.user?.display_name ||
        authStore.user?.email?.split("@")[0] ||
        "User"
    );
});

async function handleLogout() {
    await authStore.logout();
    router.push("/login");
}
</script>

<template>
    <div class="space-y-6">
        <div class="flex items-center gap-3 mb-6">
            <router-link
                :to="backTo"
                class="material-symbols-outlined text-gray-400 hover:text-gray-600 transition-colors"
            >
                arrow_back
            </router-link>
            <div class="flex items-center gap-2">
                <span class="material-symbols-outlined text-gray-500 text-2xl"
                    >lock</span
                >
                <div>
                    <h1 class="text-2xl font-semibold text-gray-900">
                        Sessions
                    </h1>
                    <p class="text-sm text-gray-500">
                        Manage the devices signed in as
                        {{ displayName }}
                    </p>
                </div>
            </div>
        </div>

        <Card>
            <template #title>
                <div class="flex items-center gap-2">
                    <span class="material-symbols-outlined text-purple-500"
                        >devices</span
                    >
                    <span>Active Sessions</span>
                </div>
            </template>
            <template #content>
                <SessionsPanel @revoked-all="router.push('/login')" />
            </template>
        </Card>

        <Card>
            <template #title>
                <div class="flex items-center gap-2">
                    <span class="material-symbols-outlined text-orange-500"
                        >settings_power</span
                    >
                    <span>Actions</span>
                </div>
            </template>
            <template #content>
                <div class="flex items-center justify-between">
                    <div>
                        <p class="text-sm font-medium">Sign out</p>
                        <p class="text-xs text-gray-500">
                            End your current session
                        </p>
                    </div>
                    <Button
                        label="Logout"
                        icon="pi pi-sign-out"
                        severity="secondary"
                        @click="handleLogout"
                    />
                </div>
            </template>
        </Card>
    </div>
</template>
