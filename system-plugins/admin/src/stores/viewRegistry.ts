import { defineStore } from 'pinia'
import { ref, computed, shallowRef } from 'vue'
import type { Component } from 'vue'
import { usePluginsStore } from './plugins'

export interface PluginViewEntry {
  pluginSlug: string
  name: string
  label: string
  component: Component
  settings?: Component
}

export const useViewRegistryStore = defineStore('viewRegistry', () => {
  const viewsByKey = shallowRef<Map<string, PluginViewEntry>>(new Map())
  const loading = ref(false)
  const ready = ref(false)
  const error = ref<string | null>(null)

  const allViews = computed(() => Array.from(viewsByKey.value.values()))

  function getView(key: string): PluginViewEntry | undefined {
    return viewsByKey.value.get(key)
  }

  async function discoverViews() {
    if (loading.value || ready.value) return
    loading.value = true
    error.value = null

    const pluginsStore = usePluginsStore()
    if (pluginsStore.plugins.length === 0) {
      await pluginsStore.fetchPlugins()
    }

    const enabledPlugins = pluginsStore.plugins.filter(p => p.status === 'enabled')
    const newMap = new Map<string, PluginViewEntry>()

    const fetchPromises = enabledPlugins.map(async (plugin) => {
      try {
        const assets = await pluginsStore.fetchPluginAssets(plugin.name)
        if (!assets?.js) return

        const blob = new Blob([assets.js], { type: 'application/javascript' })
        const url = URL.createObjectURL(blob)
        const module = await import(/* @vite-ignore */ url)
        URL.revokeObjectURL(url)

        const manifest = module.default
        if (!manifest?.views?.length) return

        for (const view of manifest.views) {
          const key = `${plugin.name}:${view.name}`
          newMap.set(key, {
            pluginSlug: plugin.name,
            name: view.name,
            label: view.label,
            component: view.component,
          })
        }
      } catch (e) {
        console.warn(`[viewRegistry] Failed to load views from plugin "${plugin.name}":`, e)
      }
    })

    await Promise.all(fetchPromises)
    viewsByKey.value = newMap
    ready.value = true
    loading.value = false
  }

  function reset() {
    viewsByKey.value = new Map()
    ready.value = false
    loading.value = false
    error.value = null
  }

  return {
    viewsByKey, loading, ready, error,
    allViews, getView, discoverViews, reset,
  }
})
