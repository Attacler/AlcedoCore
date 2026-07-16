import * as vue from 'vue'
// Expose Vue globals for plugin page-compiler (page-compiler assets reference window.vue)
if (!(window as any).vue) { (window as any).vue = vue }
import { createPinia } from 'pinia'
import PrimeVue from 'primevue/config'
import Aura from '@primevue/themes/aura'
import App from './App.vue'
import router from './router'
import 'primeicons/primeicons.css'
import './style.css'
import FilterBuilder from './components/FilterBuilder.vue'
// Expose FilterBuilder globally for plugin pages to use
if (!(window as any).FilterBuilder) { (window as any).FilterBuilder = FilterBuilder }

// Intercept 401 responses globally — redirect to login on session expiry
const origFetch = window.fetch.bind(window)
window.fetch = (input: RequestInfo | URL, init?: RequestInit) => {
  return origFetch(input, init).then(res => {
    if (res.status === 401 && !res.url.includes('/api/auth/login')) {
      window.location.hash = '#/login'
    }
    return res
  })
}

const app = vue.createApp(App)
app.use(createPinia())
app.use(router)
app.use(PrimeVue, {
  theme: {
    preset: Aura,
    options: {
      darkModeSelector: false,
      cssLayer: {
        name: 'primevue',
        order: 'tailwind-base, primevue, tailwind-utilities',
      },
    },
  },
})

// Pre-load PrimeVue component CSS for dynamically loaded plugin pages.
// Plugin pages are compiled as separate bundles (by page-compiler) that
// contain isolated copies of @primeuix/styled. Their Theme singleton lacks
// the Aura preset, so component CSS variables (from getComponentTheme().css)
// are never injected. Pre-loading here, using the admin's Theme with Aura,
// makes the complete CSS (variables + styles) available globally so plugin
// components render correctly.
import SelectStyle from 'primevue/select/style'
import TabsStyle from 'primevue/tabs/style'
import TabListStyle from 'primevue/tablist/style'
import TabStyle from 'primevue/tab/style'
import TabPanelsStyle from 'primevue/tabpanels/style'
import TabPanelStyle from 'primevue/tabpanel/style'
import MessageStyle from 'primevue/message/style'
import TagStyle from 'primevue/tag/style'
import DataTableStyle from 'primevue/datatable/style'
import ColumnStyle from 'primevue/column/style'

type PluginStyle = Record<string, any>

function preloadComponentStyle(style: PluginStyle, name: string) {
  const theme = style.getComponentTheme?.() || {}
  if (theme.css) style.load(theme.css, { name: `${name}-variables` })
  if (theme.style) style.loadStyle?.({ name: `${name}-style` }, theme.style)
}

preloadComponentStyle(SelectStyle, 'select')
preloadComponentStyle(TabsStyle, 'tabs')
preloadComponentStyle(TabListStyle, 'tablist')
preloadComponentStyle(TabStyle, 'tab')
preloadComponentStyle(TabPanelsStyle, 'tabpanels')
preloadComponentStyle(TabPanelStyle, 'tabpanel')
preloadComponentStyle(MessageStyle, 'message')
preloadComponentStyle(TagStyle, 'tag')
preloadComponentStyle(DataTableStyle, 'datatable')
preloadComponentStyle(ColumnStyle, 'column')

app.mount('#app')