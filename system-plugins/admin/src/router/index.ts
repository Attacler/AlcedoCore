import { createRouter, createWebHashHistory, RouteRecordRaw } from "vue-router";
import { useAuthStore } from "@/stores/authStore";
import LoginView from "@/views/LoginView.vue";
import Dashboard from "@/views/Dashboard.vue";
import PluginList from "@/views/PluginList.vue";
import PluginCreate from "@/views/PluginCreate.vue";
import PluginDetail from "@/views/PluginDetail.vue";
import PluginSettings from "@/views/PluginSettings.vue";
import PluginPage from "@/views/PluginPage.vue";
import RegistryList from "@/views/RegistryList.vue";
import RegistryDetail from "@/views/RegistryDetail.vue";
import CollectionList from "@/views/CollectionList.vue";
import CollectionData from "@/views/CollectionData.vue";
import RecordDetail from "@/views/RecordDetail.vue";
import PoliciesIndex from "@/views/PoliciesIndex.vue";
import PolicyDetail from "@/views/PolicyDetail.vue";
import UsersIndex from "@/views/UsersIndex.vue";
import UserDetail from "@/views/UserDetail.vue";
import RolesIndex from "@/views/RolesIndex.vue";
import RoleDetail from "@/views/RoleDetail.vue";
import SettingsIndex from "@/views/Settings/SettingsIndex.vue";
import SettingsActivity from "@/views/Settings/SettingsActivity.vue";
import SettingsCategory from "@/views/Settings/SettingsCategory.vue";
import MediaLibrary from "@/views/MediaLibrary.vue";
import ApiDocs from "@/views/ApiDocs.vue";
import CollectionBuilder from "@/views/CollectionBuilder.vue";

const routes: RouteRecordRaw[] = [
    {
        path: "/login",
        name: "Login",
        component: LoginView,
        meta: { public: true },
    },
    {
        path: "/",
        redirect: "/dashboard",
    },
    {
        path: "/dashboard",
        name: "Dashboard",
        component: Dashboard,
    },
    {
        path: "/plugins",
        name: "PluginList",
        component: PluginList,
    },
    {
        path: "/plugins/new",
        name: "PluginCreate",
        component: PluginCreate,
    },
    {
        path: "/plugins/:name",
        name: "PluginDetail",
        component: PluginDetail,
    },
    {
        path: "/plugins/:name/settings",
        name: "PluginSettings",
        component: PluginSettings,
    },
    {
        path: "/p/:plugin/:pathMatch(.*)*",
        name: "PluginPage",
        component: PluginPage,
    },
    {
        path: "/registries",
        name: "RegistryList",
        component: RegistryList,
    },
    {
        path: "/registries/new",
        name: "RegistryNew",
        component: RegistryDetail,
    },
    {
        path: "/registries/:id",
        name: "RegistryDetail",
        component: RegistryDetail,
        props: true,
    },
    {
        path: "/collections",
        name: "CollectionList",
        component: CollectionList,
    },
    {
        path: "/collections/:name/edit",
        name: "CollectionBuilder",
        component: CollectionBuilder,
    },
    {
        path: "/collections/:name/data",
        name: "CollectionData",
        component: CollectionData,
    },
    {
        path: "/detail/:collection/:id",
        name: "RecordDetail",
        component: RecordDetail,
        props: true,
    },
    {
        path: "/policies",
        name: "Policies",
        component: PoliciesIndex,
    },
    {
        path: "/policies/:id",
        name: "PolicyDetail",
        component: PolicyDetail,
    },
    {
        path: "/users",
        name: "Users",
        component: UsersIndex,
    },
    {
        path: "/users/new",
        name: "UserNew",
        component: UserDetail,
    },
    {
        path: "/users/:id",
        name: "UserDetail",
        component: UserDetail,
    },
    {
        path: "/roles",
        name: "Roles",
        component: RolesIndex,
    },
    {
        path: "/roles/:id",
        name: "RoleDetail",
        component: RoleDetail,
    },
    {
        path: "/settings",
        name: "Settings",
        component: SettingsIndex,
    },
    {
        path: "/settings/activity",
        name: "SettingsActivity",
        component: SettingsActivity,
    },
    {
        path: "/settings/:category",
        name: "SettingsCategory",
        component: SettingsCategory,
        props: true,
    },
    {
        path: "/files",
        name: "MediaLibrary",
        component: MediaLibrary,
    },
    {
        path: "/files/:folderID",
        name: "MediaLibraryFolder",
        component: MediaLibrary,
    },
    {
        path: "/files/:folderID/:fileName",
        name: "MediaLibraryFolderFileDetails",
        component: MediaLibrary,
    },
    {
        path: "/apidocs",
        name: "ApiDocs",
        component: ApiDocs,
    },
    {
        path: "/menu-builder",
        redirect: "/settings/menu",
    },
];

const router = createRouter({
    history: createWebHashHistory("/admin"),
    routes,
});

router.beforeEach(async (to, _from) => {
    if (to.meta.public) return true;

    const authStore = useAuthStore();
    if (!authStore.initialized) {
        await authStore.initialize();
    }

    if (!authStore.user) {
        return { path: "/login", query: { redirect: to.fullPath } };
    }

    return true;
});

console.log(
    "[Router] Created with routes:",
    router.getRoutes().map((r) => ({ name: r.name, path: r.path })),
);

export default router;
