import { inject } from 'vue'
import { ALCEDO_SDK_KEY, type AlcedoSDK, type PageSDK, type InputWidgetSDK, type ViewTypeSDK } from './types'

declare global {
  interface Window {
    __alcedo_sdk?: AlcedoSDK
  }
}

export function useSdk(): AlcedoSDK {
  const injected = inject(ALCEDO_SDK_KEY, null)
  if (injected) return injected

  if (typeof window !== 'undefined' && window.__alcedo_sdk) {
    return window.__alcedo_sdk
  }

  throw new Error(
    '[AlcedoSDK] No SDK context found. Ensure the component is rendered within the admin app or loaded via PluginPage.'
  )
}

export function usePageSdk(): PageSDK {
  return useSdk() as unknown as PageSDK
}

export function useInputWidgetSdk(): InputWidgetSDK {
  return useSdk() as unknown as InputWidgetSDK
}

export function useViewTypeSdk(): ViewTypeSDK {
  return useSdk() as unknown as ViewTypeSDK
}
