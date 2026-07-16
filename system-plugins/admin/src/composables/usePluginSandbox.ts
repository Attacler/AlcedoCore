import { ref } from 'vue'

export interface ApiCallMessage {
  type: 'api-call'
  method: string
  url: string
  body?: unknown
  requestId: string
  pluginName: string
}

export interface ApiResponseMessage {
  type: 'api-response'
  requestId: string
  data?: unknown
  error?: string | null
}

export interface NavigateMessage {
  type: 'navigate'
  path: string
}

export interface ToastMessage {
  type: 'toast'
  message: string
  toastType: string
  duration?: number
}

export interface PluginLoadedMessage {
  type: 'plugin-loaded'
  pluginName: string
}

export interface PluginErrorMessage {
  type: 'plugin-error'
  error: string
  pluginName: string
}

export interface IframeReadyMessage {
  type: 'iframe-ready'
  pluginName: string
}

export type SandboxInboundMessage = ApiResponseMessage
export type SandboxOutboundMessage =
  | ApiCallMessage
  | NavigateMessage
  | ToastMessage
  | PluginLoadedMessage
  | PluginErrorMessage
  | IframeReadyMessage

export function usePluginSandbox() {
  const blobUrls = ref<string[]>([])

  function createBlob(content: string, type: string): string {
    const blob = new Blob([content], { type })
    const url = URL.createObjectURL(blob)
    blobUrls.value.push(url)
    return url
  }

  function revokeBlob(url: string) {
    URL.revokeObjectURL(url)
    const idx = blobUrls.value.indexOf(url)
    if (idx !== -1) blobUrls.value.splice(idx, 1)
  }

  function cleanup() {
    blobUrls.value.forEach(url => URL.revokeObjectURL(url))
    blobUrls.value = []
  }

  return {
    blobUrls,
    createBlob,
    revokeBlob,
    cleanup,
  }
}
