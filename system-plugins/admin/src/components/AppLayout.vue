<script setup lang="ts">
import { ref, computed, watch, watchEffect, onMounted, onUnmounted } from "vue";
import { usePluginsStore } from "@/stores/plugins";
import { useCollectionsStore } from "@/stores/collections";
import { useMenuStore } from "@/stores/menuStore";
import type { MenuSection, MenuItem } from "@/types/menu";
import { useSettingsStore } from "@/stores/settingsStore";
import { useViewRegistryStore } from "@/stores/viewRegistry";
import { useAuthStore } from "@/stores/authStore";
import { useRouter } from "vue-router";
import ToastContainer from "@/components/ToastContainer.vue";
import { Toast } from "primevue";

const authStore = useAuthStore(),
    router = useRouter(),
    pluginsStore = usePluginsStore(),
    collectionsStore = useCollectionsStore(),
    menuStore = useMenuStore(),
    settingsStore = useSettingsStore(),
    viewRegistry = useViewRegistryStore();

const sidebarOpen = ref(false),
    isMobile = ref(false),
    collapsed = ref(false),
    activeSection = ref<"content" | "settings">("content");

const filteredSections = computed(() => {
    if (activeSection.value === "settings") {
        return settingsSections.value;
    }
    return [...menuStore.mergedSections];
});

/** Settings-mode sidebar sections */
const settingsSections = computed<MenuSection[]>(() => {
    const sections: MenuSection[] = [];

    sections.push({
        id: "settings-general",
        label: "General",
        icon: "settings",
        visible: true,
        items: [
            {
                id: "dashboard",
                label: "Dashboard",
                icon: "home",
                route: "/dashboard",
                visible: true,
            },
            {
                id: "settings-link",
                label: "Settings",
                icon: "settings",
                route: "/settings",
                visible: true,
            },
            {
                id: "media",
                label: "Media",
                icon: "folder",
                route: "/files",
                visible: true,
            },
            {
                id: "plugins-overview",
                label: "Plugins",
                icon: "extension",
                route: "/plugins",
                visible: true,
            },
            {
                id: "registries",
                label: "Registries",
                icon: "cloud",
                route: "/registries",
                visible: true,
            },
            {
                id: "policies",
                label: "Policies",
                icon: "policy",
                route: "/policies",
                visible: true,
            },
            {
                id: "roles",
                label: "Roles",
                icon: "security",
                route: "/roles",
                visible: true,
            },
            {
                id: "users",
                label: "Users",
                icon: "group",
                route: "/users",
                visible: true,
            },
        ],
    });

    const collectionItems: MenuItem[] = [
        {
            id: "collections-overview",
            label: "Overview",
            icon: "list",
            route: "/collections",
            visible: true,
        },
    ];
    for (const c of collectionsStore.collections) {
        collectionItems.push({
            id: `collection-${c.name}-settings`,
            label: c.display_name || c.name,
            icon: "table",
            route: `/collections/${encodeURIComponent(c.name)}/edit`,
            visible: true,
        });
    }
    sections.push({
        id: "settings-collections",
        label: "Collections",
        icon: "folder",
        visible: true,
        items: collectionItems,
    });

    const pluginItems: MenuItem[] = [];
    for (const plugin of pluginsStore.plugins) {
        pluginItems.push({
            id: `plugin-${plugin.name}-settings`,
            label: plugin.name,
            icon: "extension",
            route: `/plugins/${encodeURIComponent(plugin.name)}`,
            visible: true,
        });
    }
    sections.push({
        id: "settings-plugins",
        label: "Plugins",
        icon: "extension",
        visible: true,
        items: pluginItems,
    });

    return sections;
});

async function handleLogout() {
    await authStore.logout();
    router.push("/login");
}

const branding = computed(() => ({
    siteName: settingsStore.getSettingValue("site_name") || "AlcedoCore",
    logoUrl: settingsStore.getSettingValue("logo_url"),
    faviconUrl: settingsStore.getSettingValue("favicon_url"),
}));

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

function toggleCollapse() {
    collapsed.value = !collapsed.value;
}

function switchSection(section: "content" | "settings") {
    activeSection.value = section;
}

const menuDropdownOpen = ref(false);

function switchMenu(menuId: string) {
    menuStore.setActiveMenu(menuId);
    menuDropdownOpen.value = false;
}

function handleClickOutside(e: MouseEvent) {
    if (menuDropdownOpen.value) {
        const target = e.target as HTMLElement;
        if (!target.closest(".menu-switcher-area")) {
            menuDropdownOpen.value = false;
        }
    }
}

watchEffect(() => {
    document.title = branding.value.siteName;
});

watch(
    () => branding.value.faviconUrl,
    (url) => {
        const link = document.getElementById(
            "favicon",
        ) as HTMLLinkElement | null;

        if (url) {
            if (link) link.href = url;
        } else {
            if (link) link.href = "/favicon.ico";
        }
    },
);
function visibleSectionItems(section: { items: any[]; visible: boolean }) {
    return section.items.filter((i: any) => i.visible !== false);
}

/**
 * Resolve the route for a menu item.
 * External items use their url directly (handled via router-link :to).
 * Internal items use their route path.
 */
function resolveItemRoute(item: {
    route?: string;
    url?: string;
    external?: boolean;
}) {
    if (item.external && item.url) return item.url;
    return item.route || "/";
}

onMounted(() => {
    checkMobile();
    window.addEventListener("resize", checkMobile);
    document.addEventListener("click", handleClickOutside);

    settingsStore.fetchSettings();
    collectionsStore.fetchCollections();
    viewRegistry.discoverViews();
});

onUnmounted(() => {
    window.removeEventListener("resize", checkMobile);
    document.removeEventListener("click", handleClickOutside);
});
</script>

<template>
    <div class="flex flex-col min-h-screen bg-gray-100">
        <!-- Fixed Sidebar (desktop) / Drawer (mobile) -->
        <aside
            class="fixed left-0 top-0 h-dvh bg-slate-800 text-white z-40 flex flex-col transition-all duration-300"
            :class="{
                'w-20': !isMobile && collapsed,
                'w-64': !isMobile && !collapsed,
                'w-64 translate-x-0': isMobile && sidebarOpen,
                'w-64 -translate-x-full': isMobile && !sidebarOpen,
            }"
        >
            <!-- Logo/Brand area with collapse toggle -->
            <div
                class="h-16 flex items-center justify-between px-3 border-b border-slate-700"
            >
                <div class="flex items-center gap-2 min-w-0">
                    <img
                        v-if="branding.logoUrl"
                        :src="branding.logoUrl"
                        alt="Logo"
                        class="h-8 w-auto max-w-w-40 object-contain"
                    />
                    <span v-else class="material-symbols-outlined text-2xl"
                        >dashboard</span
                    >
                    <span
                        v-if="!collapsed || isMobile"
                        class="text-sm font-semibold truncate"
                        >{{ branding.siteName }}</span
                    >
                </div>
                <Button
                    v-if="!isMobile"
                    :icon="
                        collapsed ? 'pi pi-chevron-right' : 'pi pi-chevron-left'
                    "
                    text
                    rounded
                    class="p-1.5 text-white"
                    @click="toggleCollapse"
                    :title="collapsed ? 'Expand sidebar' : 'Collapse sidebar'"
                />
            </div>

            <!-- Menu Switcher — only in Browse mode when multiple menus exist -->
            <template
                v-if="
                    activeSection == 'content' &&
                    menuStore.menus.length > 1 &&
                    (!collapsed || isMobile)
                "
            >
                <div
                    class="px-2 py-2 border-b border-slate-700/50 menu-switcher-area"
                >
                    <div class="relative">
                        <button
                            @click="menuDropdownOpen = !menuDropdownOpen"
                            class="flex items-center gap-2 w-full px-2 py-1.5 rounded hover:bg-slate-700/50 text-sm text-slate-200 transition-colors"
                        >
                            <span class="material-symbols-outlined text-lg">{{
                                menuStore.activeMenu?.icon || "menu"
                            }}</span>
                            <span
                                v-if="!collapsed || isMobile"
                                class="flex-1 text-left truncate"
                                >{{
                                    menuStore.activeMenu?.name || "No Menu"
                                }}</span
                            >
                            <span
                                v-if="!collapsed"
                                class="material-symbols-outlined text-sm ml-auto"
                                >arrow_drop_down</span
                            >
                        </button>
                        <!-- Dropdown -->
                        <div
                            v-if="menuDropdownOpen"
                            class="absolute left-0 right-0 top-full mt-1 bg-slate-800 border border-slate-600 rounded-lg shadow-lg z-50 overflow-hidden"
                        >
                            <div
                                v-for="m in menuStore.menus"
                                :key="m.id"
                                @click="switchMenu(m.id)"
                                class="flex items-center gap-2 px-3 py-2.5 hover:bg-slate-700 cursor-pointer text-sm transition-colors"
                                :class="{
                                    'bg-slate-700/70':
                                        m.id === menuStore.activeMenuId,
                                }"
                            >
                                <span
                                    class="material-symbols-outlined text-lg"
                                    >{{ m.icon }}</span
                                >
                                <span>{{ m.name }}</span>
                                <span
                                    v-if="m.id === menuStore.activeMenuId"
                                    class="ml-auto text-xs text-blue-400"
                                    >Active</span
                                >
                            </div>
                        </div>
                    </div>
                </div>
            </template>

            <!-- Navigation - Dynamic Menu Sections -->
            <nav class="flex flex-col gap-1 p-2 flex-1 overflow-y-auto">
                <!-- Render sections filtered by active section toggle -->
                <div
                    v-for="section in filteredSections"
                    :key="section.id"
                    class="mb-2"
                >
                    <!-- Section Header -->
                    <div
                        v-if="!collapsed || isMobile"
                        class="flex items-center gap-2 px-3 py-1.5 text-xs font-semibold text-slate-400 uppercase tracking-wider"
                    >
                        <span class="material-symbols-outlined text-sm">{{
                            section.icon
                        }}</span>
                        <span>{{ section.label }}</span>
                    </div>

                    <!-- Section Items -->
                    <component
                        v-for="item in visibleSectionItems(section)"
                        :is="item.external ? 'a' : 'router-link'"
                        :key="item.id"
                        :to="resolveItemRoute(item)"
                        :href="resolveItemRoute(item)"
                        :target="item.external ? '_blank' : undefined"
                        class="flex items-center gap-2 px-3 py-2 rounded-lg hover:bg-slate-700 transition-colors border-l-2 border-transparent"
                        :class="{
                            'flex-col': !isMobile && !collapsed,
                            'text-sm': isMobile,
                            'text-slate-400': !item.visible,
                        }"
                        active-class="bg-slate-700 border-l-2 border-blue-400"
                        @click="closeSidebarOnMobile"
                    >
                        <span
                            class="material-symbols-outlined text-xl"
                            :class="{ 'mx-auto': collapsed && !isMobile }"
                            >{{ item.icon }}</span
                        >
                        <span
                            v-if="!collapsed || isMobile"
                            class="text-xs flex items-center"
                            >{{ item.label }}
                            <span
                                v-if="item.external"
                                class="material-symbols-outlined text-xs text-slate-400 ml-1"
                                >open_in_new</span
                            ></span
                        >
                    </component>

                    <!-- Empty section placeholder -->
                    <div
                        v-if="
                            section.visible &&
                            visibleSectionItems(section).length === 0 &&
                            (!collapsed || isMobile)
                        "
                        class="px-3 py-2 text-xs text-slate-500 italic"
                    >
                        No items
                    </div>
                </div>
            </nav>

            <!-- User Info & Logout -->
            <div
                v-if="authStore.user"
                class="border-t border-slate-700 px-3 py-2"
            >
                <div
                    class="flex items-center gap-2"
                    :class="{ 'justify-center': collapsed && !isMobile }"
                >
                    <div
                        class="w-8 h-8 rounded-full bg-blue-500 flex items-center justify-center text-white text-sm font-medium shrink-0"
                    >
                        {{ authStore.userInitial }}
                    </div>
                    <div v-if="!collapsed || isMobile" class="flex-1 min-w-0">
                        <p class="text-sm font-medium text-white truncate">
                            {{ authStore.displayName }}
                        </p>
                        <p class="text-xs text-slate-400 truncate">
                            {{ authStore.user.email }}
                        </p>
                    </div>
                    <Button
                        v-if="!collapsed || isMobile"
                        icon="pi pi-sign-out"
                        text
                        rounded
                        class="text-slate-400 hover:text-white"
                        @click="handleLogout"
                        title="Logout"
                    />
                </div>
            </div>

            <!-- Bottom Section Switcher (always visible, outside scrollable nav) -->
            <div class="border-t border-slate-700 p-2">
                <div :class="collapsed ? 'flex flex-col gap-1' : 'flex gap-1'">
                    <button
                        @click="switchSection('content')"
                        class="flex-1 flex items-center justify-center gap-1 px-2 py-2 rounded-lg text-xs transition-colors cursor-pointer"
                        :class="
                            activeSection === 'content'
                                ? 'bg-slate-700 text-white'
                                : 'text-slate-400 hover:bg-slate-700 hover:text-white'
                        "
                    >
                        <span class="material-symbols-outlined text-lg"
                            >browse</span
                        >
                        <span v-if="!collapsed || isMobile">Browse</span>
                    </button>
                    <button
                        v-if="
                            authStore.scopes.some(
                                (s) =>
                                    s === 'users.all' ||
                                    s === 'settings.all' ||
                                    s === 'settings.read.all' ||
                                    s === 'settings.write.all',
                            )
                        "
                        @click="switchSection('settings')"
                        class="flex-1 flex items-center justify-center gap-1 px-2 py-2 rounded-lg text-xs transition-colors cursor-pointer"
                        :class="
                            activeSection === 'settings'
                                ? 'bg-slate-700 text-white'
                                : 'text-slate-400 hover:bg-slate-700 hover:text-white'
                        "
                    >
                        <span class="material-symbols-outlined text-lg"
                            >settings</span
                        >
                        <span v-if="!collapsed || isMobile">Settings</span>
                    </button>
                </div>
            </div>
        </aside>

        <!-- Mobile header with hamburger and section switcher -->
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
        <div
            class="flex-1 flex flex-col min-h-dvh transition-all duration-300"
            :class="{
                'lg:ml-20': !isMobile && collapsed,
                'lg:ml-64': !isMobile && !collapsed,
            }"
        >
            <!-- Scrollable Content -->
            <main class="flex-1 p-4 flex flex-col grow">
                <router-view />
            </main>
        </div>
    </div>

    <!-- Toast notifications -->
    <Toast />
    <ToastContainer />
    <ConfirmDialog />
</template>
