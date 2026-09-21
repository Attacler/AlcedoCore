<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watchEffect } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "@/stores/authStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useCollectionsStore } from "@/stores/collections";
import ToastContainer from "@/components/ToastContainer.vue";
import { Toast } from "primevue";
import ConfirmDialog from "primevue/confirmdialog";

const authStore = useAuthStore(),
    settingsStore = useSettingsStore(),
    collectionsStore = useCollectionsStore(),
    router = useRouter();

interface GlobalNavItem {
    label: string;
    icon: string;
    route: string;
}

const navItems: GlobalNavItem[] = [
    { label: "Apps", icon: "apps", route: "/apps" },
    { label: "Versions", icon: "layers", route: "/versions" },
    { label: "Users", icon: "group", route: "/users" },
    { label: "Registries", icon: "cloud", route: "/registries" },
    { label: "Plugins", icon: "extension", route: "/plugins" },
];

const visibleNavItems = computed(() =>
    authStore.isAdmin
        ? navItems
        : navItems.filter((item) => item.route === "/apps"),
);

const branding = computed(() => ({
    siteName: settingsStore.getSettingValue("site_name") || "AlcedoCore",
    logoUrl: settingsStore.getSettingValue("logo_url"),
}));

const sidebarOpen = ref(false),
    isMobile = ref(false);

function checkMobile() {
    isMobile.value = window.innerWidth < 1024;
}

function toggleSidebar() {
    sidebarOpen.value = !sidebarOpen.value;
}

function closeSidebar() {
    sidebarOpen.value = false;
}

function closeSidebarOnMobile() {
    if (isMobile.value) {
        sidebarOpen.value = false;
    }
}

async function handleLogout() {
    await authStore.logout();
    router.push("/login");
}

watchEffect(() => {
    document.title = branding.value.siteName;
});

onMounted(() => {
    checkMobile();
    window.addEventListener("resize", checkMobile);
    settingsStore.fetchPlatformSettings();
});

onUnmounted(() => {
    window.removeEventListener("resize", checkMobile);
});
</script>

<template>
    <div class="flex flex-col min-h-screen bg-gray-100">
        <!-- Fixed Sidebar (desktop) / Drawer (mobile) -->
        <aside
            class="fixed left-0 top-0 h-dvh w-64 bg-slate-800 text-white z-40 flex flex-col transition-transform duration-300"
            :class="
                isMobile && !sidebarOpen ? '-translate-x-full' : 'translate-x-0'
            "
        >
            <!-- Logo/Brand area -->
            <div
                class="h-16 flex items-center gap-2 px-3 border-b border-slate-700"
            >
                <img
                    v-if="branding.logoUrl"
                    :src="branding.logoUrl"
                    alt="Logo"
                    class="h-8 w-auto max-w-40 object-contain"
                />
                <span v-else class="material-symbols-outlined text-2xl"
                    >apps</span
                >
                <span class="text-sm font-semibold truncate">{{
                    branding.siteName
                }}</span>
            </div>

            <!-- Navigation -->
            <nav class="flex flex-col gap-1 p-2 flex-1 overflow-y-auto">
                <RouterLink
                    v-for="item in visibleNavItems"
                    :key="item.route"
                    :to="item.route"
                    class="flex items-center gap-2 px-3 py-2 rounded-lg hover:bg-slate-700 transition-colors text-slate-300 border-l-2 border-transparent"
                    active-class="bg-slate-700 border-l-2 border-blue-400 text-white"
                    @click="closeSidebarOnMobile"
                >
                    <span class="material-symbols-outlined text-xl">{{
                        item.icon
                    }}</span>
                    <span class="text-sm">{{ item.label }}</span>
                </RouterLink>
            </nav>

            <!-- User Info & Logout -->
            <div
                v-if="authStore.user"
                class="border-t border-slate-700 px-3 py-2"
            >
                <div class="flex items-center gap-2">
                    <div
                        class="w-8 h-8 rounded-full bg-blue-500 flex items-center justify-center text-white text-sm font-medium shrink-0"
                    >
                        {{ authStore.userInitial }}
                    </div>
                    <div class="flex-1 min-w-0">
                        <p class="text-sm font-medium text-white truncate">
                            {{ authStore.displayName }}
                        </p>
                        <p class="text-xs text-slate-400 truncate">
                            {{ authStore.user.email }}
                        </p>
                    </div>
                    <Button
                        icon="pi pi-sign-out"
                        text
                        rounded
                        class="text-slate-400 hover:text-white"
                        title="Logout"
                        @click="handleLogout"
                    />
                </div>
            </div>
        </aside>

        <!-- Mobile header -->
        <header class="lg:hidden sticky top-0 z-30 bg-white shadow-sm">
            <div class="h-14 flex items-center px-4 gap-3">
                <button
                    @click="toggleSidebar"
                    class="p-2 -ml-2 rounded-lg hover:bg-gray-100 cursor-pointer"
                >
                    <span class="material-symbols-outlined text-xl">menu</span>
                </button>
                <h1 class="text-lg font-semibold text-gray-800">
                    {{ branding.siteName }}
                </h1>
            </div>
        </header>

        <!-- Overlay for mobile sidebar -->
        <div
            v-if="isMobile && sidebarOpen"
            class="fixed inset-0 bg-black/50 z-30 lg:hidden"
            @click="closeSidebar"
        ></div>

        <!-- Main Content Area -->
        <div class="flex-1 flex flex-col min-h-dvh lg:ml-64">
            <main class="flex-1 flex flex-col grow p-4">
                <router-view />
            </main>
        </div>
    </div>

    <!-- Toast notifications -->
    <Toast />
    <ToastContainer />
    <ConfirmDialog />
</template>
