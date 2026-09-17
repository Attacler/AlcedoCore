import { createRouter, createWebHashHistory, RouteRecordRaw } from "vue-router";
import { useAuthStore } from "@/stores/authStore";
import { useAppContextStore } from "@/stores/appContext";
import { setAppHeaders } from "@/utils/appHeaders";
import LoginView from "@/pages/LoginView.vue";
import AppsOverview from "@/pages/AppsOverview.vue";
import VersionsPage from "@/pages/VersionsPage.vue";
import VersionDetail from "@/pages/VersionDetail.vue";
import AppZoneLayout from "@/pages/AppZoneLayout.vue";
import Dashboard from "@/pages/Dashboard.vue";
import PluginList from "@/pages/plugins/PluginList.vue";
import PluginCreate from "@/pages/plugins/PluginCreate.vue";
import PluginDetail from "@/pages/plugins/PluginDetail.vue";
import PluginSettings from "@/pages/plugins/PluginSettings.vue";
import PluginPage from "@/pages/plugins/PluginPage.vue";
import RegistryList from "@/pages/RegistryList.vue";
import RegistryDetail from "@/pages/RegistryDetail.vue";
import CollectionList from "@/pages/CollectionList.vue";
import CollectionData from "@/pages/CollectionData.vue";
import RecordDetail from "@/pages/RecordDetail.vue";
import PoliciesIndex from "@/pages/PoliciesIndex.vue";
import PolicyDetail from "@/pages/PolicyDetail.vue";
import UsersIndex from "@/pages/UsersIndex.vue";
import UserDetail from "@/pages/UserDetail.vue";
import RolesIndex from "@/pages/RolesIndex.vue";
import RoleDetail from "@/pages/RoleDetail.vue";
import SettingsIndex from "@/pages/Settings/SettingsIndex.vue";
import SettingsCategory from "@/pages/Settings/SettingsCategory.vue";
import MediaLibrary from "@/pages/MediaLibrary.vue";
import ApiDocs from "@/pages/ApiDocs.vue";
import CollectionBuilder from "@/pages/CollectionBuilder.vue";

const APP_DASHBOARD_ROUTE = "AppDashboard";

const routes: RouteRecordRaw[] = [
    {
        path: "/login",
        name: "Login",
        component: LoginView,
        meta: { public: true },
    },
    {
        path: "/",
        redirect: "/apps",
    },
    // ---------------------------------------------------------------------
    // Global zone (no app context)
    // ---------------------------------------------------------------------
    {
        path: "/apps",
        name: "AppsOverview",
        component: AppsOverview,
    },
    {
        path: "/versions",
        name: "Versions",
        component: VersionsPage,
        meta: { global: true },
    },
    {
        path: "/versions/:id",
        name: "VersionDetail",
        component: VersionDetail,
        meta: { global: true },
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
    // ---------------------------------------------------------------------
    // App zone (scoped to /app/:appSlug/:version)
    // ---------------------------------------------------------------------
    {
        path: "/app/:appSlug/:version",
        component: AppZoneLayout,
        meta: { appZone: true },
        children: [
            {
                path: "",
                redirect: (to) => ({
                    name: APP_DASHBOARD_ROUTE,
                    params: to.params,
                }),
            },
            {
                path: "dashboard",
                name: APP_DASHBOARD_ROUTE,
                component: Dashboard,
            },
            {
                path: "collections",
                name: "AppCollectionList",
                component: CollectionList,
            },
            {
                path: "collections/:name/edit",
                name: "AppCollectionBuilder",
                component: CollectionBuilder,
                meta: { fullPage: true },
            },
            {
                path: "collections/:name/data",
                name: "AppCollectionData",
                component: CollectionData,
            },
            {
                path: "detail/:collection/:id",
                name: "AppRecordDetail",
                component: RecordDetail,
                props: true,
            },
            {
                path: "policies",
                name: "AppPolicies",
                component: PoliciesIndex,
            },
            {
                path: "policies/:id",
                name: "AppPolicyDetail",
                component: PolicyDetail,
            },
            {
                path: "roles",
                name: "AppRoles",
                component: RolesIndex,
            },
            {
                path: "roles/:id",
                name: "AppRoleDetail",
                component: RoleDetail,
            },
            {
                path: "settings",
                name: "AppSettings",
                component: SettingsIndex,
            },
            {
                path: "settings/:category",
                name: "AppSettingsCategory",
                component: SettingsCategory,
                props: true,
            },
            {
                path: "files",
                name: "AppMediaLibrary",
                component: MediaLibrary,
            },
            {
                path: "files/:folderID",
                name: "AppMediaLibraryFolder",
                component: MediaLibrary,
            },
            {
                path: "files/:folderID/:fileName",
                name: "AppMediaLibraryFolderFileDetails",
                component: MediaLibrary,
            },
            {
                path: "plugins",
                name: "AppPlugins",
                component: PluginList,
            },
            {
                path: "plugins/new",
                name: "AppPluginCreate",
                component: PluginCreate,
            },
            {
                path: "plugins/:name",
                name: "AppPluginDetail",
                component: PluginDetail,
            },
            {
                path: "plugins/:name/settings",
                name: "AppPluginSettings",
                component: PluginSettings,
            },
            {
                path: "apidocs",
                name: "AppApiDocs",
                component: ApiDocs,
            },
            {
                path: "p/:plugin/:pathMatch(.*)*",
                name: "AppPluginPage",
                component: PluginPage,
            },
        ],
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

    const appContext = useAppContextStore();
    const appSlug = (to.params.appSlug as string) || null;
    const version = (to.params.version as string) || null;
    appContext.setContext(appSlug, version);
    setAppHeaders(appSlug, version);

    return true;
});

console.log(
    "[Router] Created with routes:",
    router.getRoutes().map((r) => ({ name: r.name, path: r.path })),
);

export default router;
