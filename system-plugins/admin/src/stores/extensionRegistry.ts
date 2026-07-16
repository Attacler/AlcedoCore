/**
 * Extension Registry Store — Runtime Plugin Extension Registry
 *
 * Replaces the static DISPLAY_TYPE_REGISTRY pattern with a runtime registry
 * that plugins can register input widgets and view types into at runtime.
 * Supports per-plugin scoping via pluginSlug and cleanup on plugin uninstall.
 *
 * ## Usage
 *
 * ```ts
 * import { useExtensionRegistryStore } from '@/stores/extensionRegistry'
 *
 * const registry = useExtensionRegistryStore()
 *
 * // Register a plugin input widget
 * registry.registerInputWidget({
 *   type: 'star-rating',
 *   label: 'Star Rating',
 *   supportedFieldTypes: ['int'],
 *   component: () => import('@/plugins/ratings/StarRating.vue'),
 *   pluginSlug: 'ratings-plugin',
 * })
 *
 * // Register a plugin view type
 * registry.registerViewType({
 *   type: 'calendar',
 *   label: 'Calendar View',
 *   icon: 'pi pi-calendar',
 *   component: () => import('@/plugins/calendar/CalendarView.vue'),
 *   pluginSlug: 'calendar-plugin',
 * })
 *
 * // Cleanup on plugin uninstall
 * registry.unregisterPlugin('ratings-plugin')
 * ```
 */

import { defineStore } from 'pinia'
import { reactive, computed } from 'vue'
import type { FieldType } from '@/stores/collections'

/** Registration for a plugin-provided input widget */
export interface InputWidgetRegistration {
  type: string
  label: string
  supportedFieldTypes: FieldType[]
  component: () => Promise<any>
  settingsComponent?: () => Promise<any>
  pluginSlug: string
}

/** Registration for a plugin-provided view type */
export interface ViewTypeRegistration {
  type: string
  label: string
  icon?: string
  component: () => Promise<any>
  settingsComponent?: () => Promise<any>
  settings?: () => Promise<any>
  pluginSlug: string
}

/** Registration for a plugin-provided sidebar navigation item */
export interface NavItemRegistration {
  id: string
  label: string
  icon: string
  /** Internal vue-router path (e.g., "/collections") — mutually exclusive with url */
  route?: string
  /** External URL (e.g., "https://docs.example.com") — mutually exclusive with route */
  url?: string
  /** True when item links to an external URL (shows external link icon, opens new tab) */
  external?: boolean
  /** Which menu section this item should appear in (e.g., "settings") */
  sectionId: string
  /** The plugin that registered this item — used for scoping and cleanup */
  pluginSlug: string
}

export const useExtensionRegistryStore = defineStore('extensionRegistry', () => {
  /** Internal registry of plugin input widgets, keyed by unique type string */
  const inputWidgetRegistry = reactive(new Map<string, InputWidgetRegistration>())

  /** Internal registry of plugin view types, keyed by unique type string */
  const viewTypeRegistry = reactive(new Map<string, ViewTypeRegistration>())

  /**
   * Internal registry of plugin nav items, keyed by composite key "pluginSlug/id".
   * Composite key prevents cross-plugin collision per MENU-06.
   */
  const navItemRegistry = reactive(new Map<string, NavItemRegistration>())

  /**
   * Register a plugin input widget.
   * Overwrites any existing registration with the same type key.
   */
  function registerInputWidget(registration: InputWidgetRegistration): void {
    inputWidgetRegistry.set(registration.type, registration)
  }

  /**
   * Register a plugin view type.
   * Overwrites any existing registration with the same type key.
   */
  function registerViewType(registration: ViewTypeRegistration): void {
    viewTypeRegistry.set(registration.type, registration)
  }

  /**
   * Register a plugin nav item.
   * Uses composite key "pluginSlug/id" to prevent cross-plugin collision.
   * Two plugins can register nav items with the same `id` without conflict.
   */
  function registerNavItem(registration: NavItemRegistration): void {
    const key = `${registration.pluginSlug}/${registration.id}`
    navItemRegistry.set(key, registration)
  }

  /**
   * Unregister a single nav item by its composite key.
   */
  function unregisterNavItem(pluginSlug: string, id: string): void {
    const key = `${pluginSlug}/${id}`
    navItemRegistry.delete(key)
  }

  /** Look up an input widget registration by type key */
  function getInputWidget(type: string): InputWidgetRegistration | undefined {
    return inputWidgetRegistry.get(type)
  }

  /** Look up a view type registration by type key */
  function getViewType(type: string): ViewTypeRegistration | undefined {
    return viewTypeRegistry.get(type)
  }

  /**
   * Get all input widget registrations that support a given field type.
   * Used to filter available display options per field type.
   */
  function getInputWidgetsForFieldType(fieldType: FieldType): InputWidgetRegistration[] {
    const results: InputWidgetRegistration[] = []
    for (const registration of inputWidgetRegistry.values()) {
      if (registration.supportedFieldTypes.includes(fieldType)) {
        results.push(registration)
      }
    }
    return results
  }

  /**
   * Get all registrations (input widgets + view types) belonging to a specific plugin.
   * Used for display in plugin detail pages and for selective cleanup.
   */
  function getPluginRegistrations(pluginSlug: string): {
    inputWidgets: InputWidgetRegistration[]
    viewTypes: ViewTypeRegistration[]
  } {
    const inputWidgets: InputWidgetRegistration[] = []
    const viewTypes: ViewTypeRegistration[] = []

    for (const reg of inputWidgetRegistry.values()) {
      if (reg.pluginSlug === pluginSlug) inputWidgets.push(reg)
    }
    for (const reg of viewTypeRegistry.values()) {
      if (reg.pluginSlug === pluginSlug) viewTypes.push(reg)
    }

    return { inputWidgets, viewTypes }
  }

  /**
   * Remove all registrations for a plugin.
   * Called when a plugin is uninstalled to prevent stale registrations.
   */
  function unregisterPlugin(slug: string): void {
    for (const [key, reg] of inputWidgetRegistry.entries()) {
      if (reg.pluginSlug === slug) inputWidgetRegistry.delete(key)
    }
    for (const [key, reg] of viewTypeRegistry.entries()) {
      if (reg.pluginSlug === slug) viewTypeRegistry.delete(key)
    }
    for (const [key, reg] of navItemRegistry.entries()) {
      if (reg.pluginSlug === slug) navItemRegistry.delete(key)
    }
  }

  /** All registered input widgets (reactive, updated on registration/removal) */
  const allInputWidgets = computed(() => Array.from(inputWidgetRegistry.values()))

  /** All registered view types (reactive, updated on registration/removal) */
  const allViewTypes = computed(() => Array.from(viewTypeRegistry.values()))

  /** All registered nav items (reactive, updated on registration/removal) */
  const allNavItems = computed(() => Array.from(navItemRegistry.values()))

  /**
   * Get all nav items registered by a specific plugin.
   * Used for selective cleanup and for the merge logic in AppLayout.
   */
  function getPluginNavItems(pluginSlug: string): NavItemRegistration[] {
    const results: NavItemRegistration[] = []
    for (const reg of navItemRegistry.values()) {
      if (reg.pluginSlug === pluginSlug) {
        results.push(reg)
      }
    }
    return results
  }

  /**
   * Get the count of nav items registered by a specific plugin.
   * Used for status display and verification.
   */
  function getPluginNavItemCount(pluginSlug: string): number {
    let count = 0
    for (const reg of navItemRegistry.values()) {
      if (reg.pluginSlug === pluginSlug) count++
    }
    return count
  }

  const allDisplayComponents = computed(() => [
    ...allInputWidgets.value,
    ...allViewTypes.value,
  ])

  function getDisplayComponent(type: string): InputWidgetRegistration | ViewTypeRegistration | undefined {
    return getInputWidget(type) || getViewType(type)
  }

  return {
    registerInputWidget,
    registerViewType,
    getInputWidget,
    getViewType,
    getInputWidgetsForFieldType,
    getPluginRegistrations,
    unregisterPlugin,
    allInputWidgets,
    allViewTypes,
    allDisplayComponents,
    getDisplayComponent,
    registerNavItem,
    unregisterNavItem,
    allNavItems,
    getPluginNavItems,
    getPluginNavItemCount,
  }
})
