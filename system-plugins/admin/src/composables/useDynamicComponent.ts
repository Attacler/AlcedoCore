/**
 * Dynamic Component Loader Composable
 *
 * Wraps Vue's `defineAsyncComponent` to provide a consistent interface
 * for lazy-loading plugin-provided components via dynamic `import()`.
 *
 * Provides built-in loading delay, timeout, and error handling so plugins
 * don't need to handle these concerns individually.
 *
 * ## Usage
 *
 * ```ts
 * import { useDynamicComponent } from '@/composables/useDynamicComponent'
 *
 * const { loadComponent } = useDynamicComponent()
 *
 * // In <script setup> or composition function:
 * const MyAsyncComponent = loadComponent({
 *   component: () => import('@/plugins/my-plugin/MyWidget.vue'),
 * })
 * ```
 *
 * The returned component can be used directly in `<component :is="...">`.
 */

import { defineAsyncComponent, defineComponent, h, type Component } from 'vue'

export interface AsyncComponentRegistration {
  /** Dynamic import factory: () => import('path/to/Component.vue') */
  component: () => Promise<any>
}

/**
 * Default loading component shown while the async component is being fetched.
 * Renders a minimal PrimeVue-style spinner placeholder.
 */
const DefaultLoadingComponent: Component = defineComponent({
  name: 'AsyncLoading',
  setup() {
    return () => h('div', { class: 'flex items-center justify-center py-8 text-gray-400 text-sm' }, [
      h('i', { class: 'pi pi-spin pi-spinner mr-2' }),
      'Loading...',
    ])
  },
})

/**
 * Default error component shown when the async component fails to load.
 */
const DefaultErrorComponent: Component = defineComponent({
  name: 'AsyncError',
  setup() {
    return () => h('div', { class: 'flex items-center justify-center py-8 text-red-500 text-sm' }, [
      h('i', { class: 'pi pi-exclamation-triangle mr-2' }),
      'Failed to load component',
    ])
  },
})

export function useDynamicComponent() {
  /**
   * Convert an async component registration into a Vue component
   * that can be used in `<component :is="...">`.
   *
   * The returned component handles:
   * - Loading state (200ms delay before showing spinner)
   * - Timeout (10s default)
   * - Error state (if import fails)
   */
  function loadComponent(registration: AsyncComponentRegistration): Component {
    return defineAsyncComponent({
      loader: registration.component,
      loadingComponent: DefaultLoadingComponent,
      errorComponent: DefaultErrorComponent,
      delay: 200,
      timeout: 10000,
    })
  }

  return { loadComponent }
}
