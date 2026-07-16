<template>
  <div class="plugin-page">
    <!-- Breadcrumb -->
    <nav class="flex items-center text-sm text-gray-500 mb-4">
      <router-link to="/dashboard" class="hover:text-gray-700">Admin</router-link>
      <span class="mx-2">/</span>
      <router-link :to="`/plugins/${pluginName}`" class="hover:text-gray-700">{{ pluginDisplayName }}</router-link>
      <span class="mx-2">/</span>
      <span class="text-gray-900">{{ pageLabel }}</span>
    </nav>

    <!-- Loading spinner -->
    <div v-if="loading" class="flex items-center justify-center p-8">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900"></div>
      <span class="ml-3 text-gray-600">Loading page...</span>
    </div>

    <!-- Component -->
    <component :is="currentComponent" v-if="!loading && !error && !notFound" />

    <!-- Error boundary -->
    <div v-if="error && !loading" class="bg-red-50 border border-red-200 rounded-lg p-4">
      <h3 class="text-red-800 font-medium flex items-center">
        <span class="mr-2">⚠</span>
        {{ errorTitle }}
      </h3>
      <p class="text-red-600 mt-1 text-sm">{{ errorMessage }}</p>
      <button @click="retry" class="mt-3 text-sm text-red-700 hover:text-red-900 underline">
        Try again
      </button>
    </div>

    <!-- 404 page -->
    <div v-if="notFound && !loading" class="text-center py-12">
      <div class="text-6xl mb-4">🔍</div>
      <h3 class="text-xl font-medium text-gray-900 mb-2">Page not found</h3>
      <p class="text-gray-500 mb-4">The page "{{ pagePath }}" does not exist in this plugin.</p>
      <router-link :to="`/plugins/${pluginName}`" class="text-blue-600 hover:text-blue-800 underline">
        Back to plugin
      </router-link>
    </div>
  </div>
</template>

<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, shallowRef, computed, watch, defineAsyncComponent } from 'vue'
import { useRoute } from 'vue-router'
import { useAlcedoClient } from '@/composables/useAlcedoClient'

const props = defineProps({
  devServerUrl: {
    type: String,
    default: '',
  },
})

const route = useRoute()
const { client, assets } = useAlcedoClient(props.devServerUrl || undefined)

const isDevMode = computed(() => !!props.devServerUrl)

const pluginName = computed(() => route.params.plugin || '')
const pagePath = computed(() => {
  const match = route.params.pathMatch
  if (!match) return '/'
  return Array.isArray(match) ? '/' + match.join('/') : '/' + match
})

const error = ref<string | null>(null)
const currentComponent = shallowRef(null)
const injectedElements: HTMLElement[] = []
const blobUrls = ref([] as string[])

// Loading and state tracking
const loading = ref(false)
const notFound = ref(false)
const pluginDisplayName = ref('')
const pageLabel = ref('')

// SSE connection state
const eventSource = ref<EventSource | null>(null)
const connected = ref(false)
const reconnectAttempts = ref(0)
const maxReconnectAttempts = 5
const baseDelay = 1000 // ms

const errorTitle = computed(() => {
  if (error.value?.includes('Failed to load')) return 'Failed to load page'
  if (error.value?.includes('not found')) return 'Page not found'
  return 'Something went wrong'
})

const errorMessage = computed(() => {
  if (error.value) return error.value.replace(/^Failed to load plugin component: /, '')
  return ''
})

const retry = () => {
  error.value = null
  notFound.value = false
  loadAssets()
}

// Fetch plugin display name for breadcrumb
const fetchPluginInfo = async () => {
  try {
    const plugins = await client.plugins.list()
    const plugin = (plugins as any[]).find(p => p.name === pluginName.value)
    pluginDisplayName.value = plugin?.display_name || plugin?.name || pluginName.value
  } catch (e) {
    pluginDisplayName.value = pluginName.value
  }
}

// Fetch page info for breadcrumb label and 404 handling
const fetchPageInfo = async (): Promise<boolean> => {
  try {
    const pages = await client.plugins.pages(pluginName.value)
    if (!Array.isArray(pages)) {
      console.warn('[PluginPage] pages response is not an array:', pages)
      return true
    }
    const page = pages.find(p => p.path === pagePath.value)
    if (!page) {
      notFound.value = true
      return false
    }
    pageLabel.value = page.label || page.path
    return true
  } catch (e) {
    // Ignore - will be handled by loadComponent
    return true
  }
}

const loadAssets = async () => {
  console.log('[PluginPage] loadAssets starting, pluginName:', pluginName.value, 'pagePath:', pagePath.value)
  // Validate plugin name (basic validation)
  if (!pluginName.value || !/^[a-zA-Z0-9_-]+$/.test(pluginName.value)) {
    error.value = 'Invalid plugin name'
    return
  }

  loading.value = true
  error.value = null
  notFound.value = false

  try {
    let css = ''

    if (isDevMode.value) {
      // Dev mode: fetch CSS from dev server
      const cssResponse = await fetch(`${props.devServerUrl}/dev/css?plugin=${encodeURIComponent(pluginName.value)}`)
      css = await cssResponse.text()
      console.log('[PluginPage] CSS loaded, length:', css.length)
    } else {
      // Production mode: use API endpoint
      const { css: apiCss } = await client.plugins.assets(pluginName.value)
      css = apiCss
    }

    if (css) {
      const style = document.createElement('style')
      style.textContent = css
      style.setAttribute('data-plugin-asset', pluginName.value)
      document.head.appendChild(style)
      injectedElements.push(style)
    }

    await loadComponent()
  } catch (e) {
    console.error('[PluginPage] error:', e)
    error.value = `Failed to load plugin component: ${e}`
  } finally {
    loading.value = false
  }
}

const loadComponent = async () => {
  try {
    let moduleUrl = '';
    if (isDevMode.value) {
      moduleUrl = `${props.devServerUrl}/dev/js?plugin=${encodeURIComponent(pluginName.value)}&page=${encodeURIComponent(pagePath.value)}`;
    } else {
      const { js } = await client.plugins.assets(pluginName.value);
      blobUrls.value.forEach(url => URL.revokeObjectURL(url));
      blobUrls.value = [];
      const blob = new Blob([js], { type: 'application/javascript' });
      moduleUrl = URL.createObjectURL(blob);
      blobUrls.value.push(moduleUrl);
    }

    const module = await import(/* @vite-ignore */ moduleUrl);
    const pageDefs = module.default || Object.values(module).find(v => Array.isArray(v));
    const pageDef = pageDefs?.find((p: any) => p.path === pagePath.value);
    const Component = pageDef?.component;

    if (Component) {
      currentComponent.value = Component;
    } else {
      error.value = `Page not found: ${pagePath.value}`;
    }
  } catch (e) {
    error.value = `Failed to load plugin component: ${e}`;
  }
};

// SSE connection functions
const connectSSE = () => {
  if (eventSource.value) {
    eventSource.value.close()
  }

  // Only connect SSE in dev mode
  if (!isDevMode.value) {
    return
  }

  const es = new EventSource(`${props.devServerUrl}/streaming`)
  eventSource.value = es

  es.onopen = () => {
    connected.value = true
    reconnectAttempts.value = 0
  }

  es.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data)
      const eventType = data.type
      const plugin = data.plugin
      const page = data.page

      if (eventType === 'reloadJS') {
        handleReloadJS(plugin, page)
      } else if (eventType === 'reloadCSS') {
        handleReloadCSS(plugin)
      }
    } catch (e) {
      console.error('Failed to parse SSE message:', e)
    }
  }

  es.onerror = () => {
    connected.value = false
    es.close()
    scheduleReconnect()
  }
}

const scheduleReconnect = () => {
  if (reconnectAttempts.value >= maxReconnectAttempts) {
    console.warn('Max SSE reconnect attempts reached')
    return
  }

  const delay = baseDelay * Math.pow(2, reconnectAttempts.value)
  reconnectAttempts.value++

  setTimeout(() => {
    connectSSE()
  }, delay)
}

const disconnectSSE = () => {
  if (eventSource.value) {
    eventSource.value.close()
    eventSource.value = null
  }
  connected.value = false
}

// handleReloadJS: unmount → fetch new bundle → remount cycle
const handleReloadJS = async (plugin, page) => {
  // Check if plugin/page match current route — if not, ignore event
  if (plugin !== pluginName.value || page !== pagePath.value) {
    return
  }

  try {
    // Track old injected elements for cleanup
    const oldInjectedElements = [...injectedElements]

    // Unmount existing component
    currentComponent.value = null
    // Wait for unmount completion, then fetch new bundle
    await nextTick()

    // Remove old injected styles
    oldInjectedElements.forEach(el => el.remove())

    // Fetch new CSS via GET /dev/css?plugin=X
    const cssResponse = await fetch(`${props.devServerUrl}/dev/css?plugin=${encodeURIComponent(plugin)}`)
    const cssText = await cssResponse.text()

    // Inject new style element with data-plugin-asset attribute
    const style = document.createElement('style')
    style.textContent = cssText
    style.setAttribute('data-plugin-asset', plugin)
    document.head.appendChild(style)
    injectedElements.push(style)

    // Fetch new JS bundle via dynamic import
    const moduleUrl = `${props.devServerUrl}/dev/js?plugin=${encodeURIComponent(plugin)}&page=${encodeURIComponent(page)}`
    const module = await import(/* @vite-ignore */ moduleUrl)

    // Extract Component from module.default or named export
    let Component = module.default
    if (!Component || typeof Component !== 'function') {
      const keys = Object.keys(module).filter(k => k !== 'default')
      Component = keys.find(k => {
        const exp = module[k]
        return exp && typeof exp === 'function' && (exp.__vither || k.endsWith('Page') || k.endsWith('Component'))
      })
      Component = Component ? module[Component] : null
    }

    if (Component) {
      currentComponent.value = Component;
    }
  } catch (e) {
    error.value = `Hot reload failed: ${e}`
  }
}

// handleReloadCSS: replace style.textContent without full component reload
const handleReloadCSS = async (plugin) => {
  // Check if plugin matches current pluginName — if not, ignore
  if (plugin !== pluginName.value) {
    return
  }

  try {
    // Find existing style element with data-plugin-asset=pluginName
    const existingStyle = document.querySelector(`style[data-plugin-asset="${plugin}"]`)

    if (existingStyle) {
      // Fetch new CSS and replace textContent
      const cssResponse = await fetch(`${props.devServerUrl}/dev/css?plugin=${encodeURIComponent(plugin)}`)
      const cssText = await cssResponse.text()
      existingStyle.textContent = cssText
    } else {
      // Fallback: inject new style element
      const cssResponse = await fetch(`${props.devServerUrl}/dev/css?plugin=${encodeURIComponent(plugin)}`)
      const cssText = await cssResponse.text()
      const style = document.createElement('style')
      style.textContent = cssText
      style.setAttribute('data-plugin-asset', plugin)
      document.head.appendChild(style)
      injectedElements.push(style)
    }
  } catch (e) {
    console.error('CSS hot reload failed:', e)
  }
}

// Load on mount and connect SSE
onMounted(async () => {
  console.log('[PluginPage] mounted, route params:', route.params, 'pathMatch:', route.params.pathMatch)

  // Fetch plugin and page info for breadcrumb
  await fetchPluginInfo()
  const pageExists = await fetchPageInfo()
  if (!pageExists) {
    return
  }

  await loadAssets()
  connectSSE()
})

// Cleanup on unmount
onUnmounted(() => {
  disconnectSSE()
  injectedElements.forEach(el => el.remove())
  injectedElements.length = 0
  blobUrls.value.forEach(url => URL.revokeObjectURL(url))
  blobUrls.value = []
})

function doimport(str: string) {
      if ((globalThis as any).URL.createObjectURL) {
        const blob = new Blob([str], { type: "text/javascript" });
        const url = URL.createObjectURL(blob);
        const module = import(/* @vite-ignore */ url);
        URL.revokeObjectURL(url); // GC objectURLs
        return module;
      }

      const url = "data:text/javascript;base64," + btoa(str);
      return import(/* @vite-ignore */ url);
    }
</script>
