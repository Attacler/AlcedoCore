<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { usePluginSandbox, type SandboxOutboundMessage, type ApiResponseMessage } from '@/composables/usePluginSandbox'

const props = defineProps({
  css: { type: String, default: '' },
  js: { type: String, default: '' },
  pagePath: { type: String, default: '/' },
  pluginName: { type: String, required: true },
  fullpage: { type: Boolean, default: false },
  vueCdnUrl: { type: String, default: 'https://esm.sh/vue@3.4' },
})

const emit = defineEmits<{
  (e: 'loaded', pluginName: string): void
  (e: 'error', error: string): void
  (e: 'navigate', path: string): void
  (e: 'toast', message: string, type: string, duration?: number): void
}>()

const iframeRef = ref<HTMLIFrameElement | null>(null)
const iframeSrc = ref('')
const ready = ref(false)

const { createBlob, cleanup } = usePluginSandbox()

function buildSandbox() {
  cleanup()
  ready.value = false

  if (!props.js && !props.css) {
    emit('error', 'No plugin assets to load')
    return
  }

  const cssBlobUrl = props.css ? createBlob(props.css, 'text/css') : null
  const jsBlobUrl = props.js ? createBlob(props.js, 'application/javascript') : null

  const bridgeScript = generateBridgeScript(props.pluginName, jsBlobUrl || '')
  const bridgeBlobUrl = createBlob(bridgeScript, 'application/javascript')

  const htmlContent = generateSandboxHtml(cssBlobUrl, bridgeBlobUrl, props.vueCdnUrl)
  const htmlBlobUrl = createBlob(htmlContent, 'text/html')

  iframeSrc.value = htmlBlobUrl
  ready.value = true
}

function generateBridgeScript(pluginName: string, jsBlobUrl: string): string {
  return `
const PLUGIN_NAME = '${pluginName}';
const JS_BLOB_URL = '${jsBlobUrl}';
const TIMEOUT_MS = 30000;

let requestIdCounter = 0;
const pendingRequests = new Map();

window.addEventListener('message', (event) => {
  const msg = event.data;
  if (msg && msg.type === 'api-response') {
    const pending = pendingRequests.get(msg.requestId);
    if (pending) {
      if (msg.error) {
        pending.reject(new Error(msg.error));
      } else {
        pending.resolve(msg.data);
      }
      pendingRequests.delete(msg.requestId);
    }
  }
});

function postAndWait(method, url, body) {
  return new Promise((resolve, reject) => {
    const requestId = 'req_' + (++requestIdCounter);
    pendingRequests.set(requestId, { resolve, reject });
    window.parent.postMessage({ type: 'api-call', method, url, body: body !== undefined ? body : undefined, requestId, pluginName: PLUGIN_NAME }, '*');
    setTimeout(() => {
      if (pendingRequests.has(requestId)) {
        pendingRequests.delete(requestId);
        reject(new Error('API call timed out: ' + method + ' ' + url));
      }
    }, TIMEOUT_MS);
  });
}

window.__alcedo_sdk = {
  request: postAndWait,
  get: (url) => postAndWait('GET', url),
  post: (url, body) => postAndWait('POST', url, body),
  put: (url, body) => postAndWait('PUT', url, body),
  patch: (url, body) => postAndWait('PATCH', url, body),
  delete: (url) => postAndWait('DELETE', url),
  navigate: (path) => {
    window.parent.postMessage({ type: 'navigate', path }, '*');
  },
  toast: {
    show: (message, type, duration) => {
      window.parent.postMessage({ type: 'toast', message, toastType: type, duration }, '*');
    }
  }
};

window.parent.postMessage({ type: 'iframe-ready', pluginName: PLUGIN_NAME }, '*');

if (JS_BLOB_URL) {
  import(JS_BLOB_URL).then((module) => {
    window.__plugin_module = module;
    window.parent.postMessage({ type: 'plugin-loaded', pluginName: PLUGIN_NAME }, '*');
  }).catch((err) => {
    window.parent.postMessage({
      type: 'plugin-error',
      error: err instanceof Error ? err.message : String(err),
      pluginName: PLUGIN_NAME
    }, '*');
  });
}
`.trim()
}

function generateSandboxHtml(
  cssBlobUrl: string | null,
  bridgeBlobUrl: string,
  vueCdn: string,
): string {
  return `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
${cssBlobUrl ? `<link rel="stylesheet" href="${cssBlobUrl}">` : ''}
<style>
  body { margin: 0; font-family: system-ui, -apple-system, sans-serif; }
  #plugin-root { min-height: 100vh; }
</style>
<script type="importmap">
{
  "imports": {
    "vue": "${vueCdn}"
  }
}
<\/script>
</head>
<body>
<div id="plugin-root"></div>
<script type="module" src="${bridgeBlobUrl}"><\/script>
</body>
</html>`
}

function handleMessage(event: MessageEvent) {
  if (event.origin !== window.location.origin) return
  if (event.source !== iframeRef.value?.contentWindow) return

  const msg = event.data as SandboxOutboundMessage
  if (!msg || typeof msg !== 'object') return

  switch (msg.type) {
    case 'api-call': {
      const iframe = iframeRef.value
      if (!iframe?.contentWindow) return
      makeApiCall(msg.method, msg.url, msg.body, msg.requestId, iframe.contentWindow, msg.pluginName)
      break
    }
    case 'navigate':
      emit('navigate', msg.path)
      break
    case 'toast':
      emit('toast', msg.message, msg.toastType, msg.duration)
      break
    case 'plugin-loaded':
      emit('loaded', msg.pluginName)
      break
    case 'plugin-error':
      emit('error', msg.error)
      break
    case 'iframe-ready':
      break
  }
}

async function makeApiCall(
  method: string,
  url: string,
  body: unknown,
  requestId: string,
  target: Window,
  pluginName: string,
) {
  try {
    const fullUrl = url.startsWith('http') ? url : `${window.location.origin}${url}`
    const fetchOptions: RequestInit = {
      method,
      headers: {
        'Content-Type': 'application/json',
        'X-Plugin-Slug': pluginName || 'plugin-sandbox',
      },
      credentials: 'include',
    }
    if (body !== undefined && body !== null && method !== 'GET' && method !== 'HEAD') {
      fetchOptions.body = JSON.stringify(body)
    }

    const response = await fetch(fullUrl, fetchOptions)
    const contentType = response.headers.get('content-type') || ''
    let data: unknown
    if (contentType.includes('application/json')) {
      data = await response.json()
    } else {
      data = await response.text()
    }

    const responseMsg: ApiResponseMessage = {
      type: 'api-response',
      requestId,
      data: response.ok ? data : null,
      error: response.ok ? null : `HTTP ${response.status}: ${JSON.stringify(data)}`,
    }
    target.postMessage(responseMsg, window.location.origin)
  } catch (err) {
    const responseMsg: ApiResponseMessage = {
      type: 'api-response',
      requestId,
      data: null,
      error: err instanceof Error ? err.message : 'Unknown error',
    }
    target.postMessage(responseMsg, window.location.origin)
  }
}

onMounted(() => {
  buildSandbox()
  window.addEventListener('message', handleMessage)
})

onUnmounted(() => {
  window.removeEventListener('message', handleMessage)
  cleanup()
})

watch(() => [props.pluginName, props.pagePath], () => {
  buildSandbox()
})

const iframeStyle = computed(() => ({
  width: '100%',
  height: props.fullpage ? '100vh' : '600px',
  border: 'none',
  borderRadius: '0',
  display: 'block',
}))
</script>

<template>
  <div :class="['plugin-sandbox', fullpage ? 'fullpage' : '']">
    <iframe
      v-if="ready && iframeSrc"
      ref="iframeRef"
      :src="iframeSrc"
      sandbox="allow-scripts allow-same-origin"
      :style="iframeStyle"
      title="Plugin sandbox"
    />
    <div v-else class="flex items-center justify-center p-8">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900"></div>
      <span class="ml-3 text-gray-600">Loading plugin...</span>
    </div>
  </div>
</template>

<style scoped>
.plugin-sandbox.fullpage {
  padding: 0;
  margin: 0;
  height: 100%;
}
.plugin-sandbox.fullpage iframe {
  height: 100vh;
}
</style>
