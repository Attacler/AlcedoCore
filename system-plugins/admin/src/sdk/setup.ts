/// <reference types="vite/client" />
import type { AlcedoSDK } from './types'

export function createStoreProxy<T extends object>(store: T): T {
  return new Proxy(store, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver)
      if (value !== null && typeof value === 'object' && !(value instanceof Promise)) {
        return createStoreProxy(value as object)
      }
      return value
    },
    set(_target, prop, _value) {
      if (import.meta.env.DEV) {
        throw new Error(
          `[AlcedoSDK] Cannot write to read-only store property "${String(prop)}". ` +
          'Plugin components cannot mutate admin stores directly.'
        )
      }
      return true
    },
    deleteProperty(_target, prop) {
      if (import.meta.env.DEV) {
        throw new Error(
          `[AlcedoSDK] Cannot delete store property "${String(prop)}". Read-only store.`
        )
      }
      return true
    },
  })
}

export interface SdkOptions {
  client: AlcedoSDK['client']
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  stores: Record<string, any>
  navigate: (path: string) => void
  toast: AlcedoSDK['toast']
  signal?: AbortSignal
}

export function createSdk(options: SdkOptions): AlcedoSDK {
  return {
    client: options.client,
    stores: {
      plugins: createStoreProxy(options.stores.plugins),
      collections: createStoreProxy(options.stores.collections),
      registries: createStoreProxy(options.stores.registries),
      theme: createStoreProxy(options.stores.theme),
    },
    navigate: options.navigate,
    toast: options.toast,
    signal: options.signal,
  }
}
