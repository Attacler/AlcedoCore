import { createRouter, createWebHashHistory, RouteRecordRaw } from 'vue-router'
import { useAuthStore } from '@/stores/authStore'

const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    name: 'Login',
    component: () => import('../views/LoginView.vue'),
    meta: { public: true },
  },
  {
    path: '/',
    redirect: '/dashboard',
  },
  {
    path: '/dashboard',
    name: 'Dashboard',
    component: () => import('../views/Dashboard.vue'),
  },
  {
    path: '/plugins',
    name: 'PluginList',
    component: () => import('../views/PluginList.vue'),
  },
  {
    path: '/plugins/new',
    name: 'PluginCreate',
    component: () => import('../views/PluginCreate.vue'),
  },
  {
    path: '/plugins/:name',
    name: 'PluginDetail',
    component: () => import('../views/PluginDetail.vue'),
  },
  {
    path: '/plugins/:name/settings',
    name: 'PluginSettings',
    component: () => import('../views/PluginSettings.vue'),
  },
  {
    path: '/p/:plugin/:pathMatch(.*)*',
    name: 'PluginPage',
    component: () => import('../views/PluginPage.vue'),
  },
  {
    path: '/registries',
    name: 'RegistryList',
    component: () => import('../views/RegistryList.vue'),
  },
  {
    path: '/registries/new',
    name: 'RegistryNew',
    component: () => import('../views/RegistryDetail.vue'),
  },
  {
    path: '/registries/:id',
    name: 'RegistryDetail',
    component: () => import('../views/RegistryDetail.vue'),
    props: true,
  },
  {
    path: '/collections',
    name: 'CollectionList',
    component: () => import('../views/CollectionList.vue'),
  },
  {
    path: '/collections/:name/edit',
    name: 'CollectionBuilder',
    component: () => import('../views/CollectionBuilder.vue'),
  },
  {
    path: '/collections/:name/data',
    name: 'CollectionData',
    component: () => import('../views/CollectionData.vue'),
  },
  {
    path: '/detail/:collection/:id',
    name: 'RecordDetail',
    component: () => import('../views/RecordDetail.vue'),
    props: true,
  },
  {
    path: '/policies',
    name: 'Policies',
    component: () => import('../views/PoliciesIndex.vue'),
  },
  {
    path: '/policies/:id',
    name: 'PolicyDetail',
    component: () => import('../views/PolicyDetail.vue'),
  },
  {
    path: '/users',
    name: 'Users',
    component: () => import('../views/UsersIndex.vue'),
  },
  {
    path: '/users/new',
    name: 'UserNew',
    component: () => import('../views/UserDetail.vue'),
  },
  {
    path: '/users/:id',
    name: 'UserDetail',
    component: () => import('../views/UserDetail.vue'),
  },
  {
    path: '/roles',
    name: 'Roles',
    component: () => import('../views/RolesIndex.vue'),
  },
  {
    path: '/roles/:id',
    name: 'RoleDetail',
    component: () => import('../views/RoleDetail.vue'),
  },
  {
    path: '/settings',
    name: 'Settings',
    component: () => import('../views/SettingsIndex.vue'),
  },
  {
    path: '/settings/activity',
    name: 'SettingsActivity',
    component: () => import('../views/SettingsActivity.vue'),
  },
  {
    path: '/settings/menus',
    name: 'MenusIndex',
    component: () => import('../views/MenusIndex.vue'),
  },
  {
    path: '/settings/:category',
    name: 'SettingsCategory',
    component: () => import('../views/SettingsCategory.vue'),
    props: true,
  },
  {
    path: '/files',
    name: 'MediaLibrary',
    component: () => import('../views/MediaLibrary.vue'),
  },
  {
    path: '/apidocs',
    name: 'ApiDocs',
    component: () => import('../views/ApiDocs.vue'),
  },
  {
    path: '/menu-builder',
    redirect: '/settings/menu',
  },
]

const router = createRouter({
  history: createWebHashHistory('/admin'),
  routes,
})

router.beforeEach(async (to, _from) => {
  if (to.meta.public) return true

  const authStore = useAuthStore()
  if (!authStore.initialized) {
    await authStore.initialize()
  }

  if (!authStore.user) {
    return { path: '/login', query: { redirect: to.fullPath } }
  }

  return true
})

console.log('[Router] Created with routes:', router.getRoutes().map(r => ({ name: r.name, path: r.path })))

export default router
