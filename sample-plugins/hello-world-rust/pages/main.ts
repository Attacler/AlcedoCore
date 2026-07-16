import IndexPage from './IndexPage.vue'
import KvDemoPage from './KvDemoPage.vue'
import SettingsPage from './SettingsPage.vue'
import DbItemsPage from './DbItemsPage.vue'

export { IndexPage, KvDemoPage, SettingsPage, DbItemsPage }

export default {
  manifestVersion: 1,
  pluginSlug: 'hello-world-rust',
  pages: [
    {
      path: '/',
      label: 'Hello',
      icon: 'home',
      sidebar: true,
      component: IndexPage,
    },
    {
      path: '/kv-demo',
      label: 'KV Demo',
      icon: 'page',
      sidebar: true,
      component: KvDemoPage,
    },
    {
      path: '/settings',
      label: 'Settings',
      icon: 'settings',
      sidebar: true,
      component: SettingsPage,
    },
    {
      path: '/items',
      label: 'Items',
      icon: 'list',
      sidebar: true,
      component: DbItemsPage,
    },
  ],
}
