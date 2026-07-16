<template>
  <div class="docs-viewer" v-if="content || loading || error" ref="viewerRef">
    <div v-if="loading" class="text-gray-500 py-8 text-center">Loading documentation...</div>
    <div v-else-if="error" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ error }}</div>
    <div v-else-if="content" class="prose" v-html="sanitizedHtml"></div>
    <div v-else class="text-gray-400 italic py-8 text-center">No documentation available</div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, nextTick } from 'vue'
import DOMPurify from 'dompurify'
import { marked } from 'marked'
import hljs from 'highlight.js'

const props = defineProps<{
  content: string
  loading?: boolean
  error?: string | null
}>()

const emit = defineEmits<{
  navigate: [path: string]
}>()

const viewerRef = ref<HTMLElement | null>(null)
const html = ref('')

marked.setOptions({
  highlight: (code: string, lang: string) => {
    if (lang && hljs.getLanguage(lang)) {
      return hljs.highlight(code, { language: lang }).value
    }
    return hljs.highlightAuto(code).value
  }
})

const sanitizedHtml = computed(() => {
  const rawHtml = html.value
  if (!rawHtml) return ''
  let allowed = DOMPurify.sanitize(rawHtml, {
    ALLOWED_TAGS: [
      'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
      'p', 'br', 'hr',
      'ul', 'ol', 'li',
      'blockquote', 'pre', 'code',
      'a', 'strong', 'em', 'del', 'ins',
      'table', 'thead', 'tbody', 'tr', 'th', 'td',
      'img', 'div', 'span'
    ],
    ALLOWED_ATTR: ['href', 'src', 'alt', 'title', 'class', 'id', 'target', 'data-original-href']
  })
  // Add data-original-href and change href to prevent navigation
  if (typeof window !== 'undefined') {
    const tempDiv = document.createElement('div')
    tempDiv.innerHTML = allowed
    const anchors = tempDiv.querySelectorAll('a')
    anchors.forEach(anchor => {
      const href = anchor.getAttribute('href')
      if (href) {
        anchor.setAttribute('data-original-href', href)
        if (href.includes('.md') || href.startsWith('./') || href.startsWith('../') || href.startsWith('/')) {
          anchor.setAttribute('href', 'javascript:void(0)')
        }
      }
    })
    allowed = tempDiv.innerHTML
  }
  return allowed
})

watch(
  () => props.content,
  (content) => {
    if (content) {
      html.value = marked.parse(content, { async: false }) as string
    } else {
      html.value = ''
    }
  },
  { immediate: true }
)

function setupLinkHandler() {
  if (!viewerRef.value) return
  console.log('[DocsViewer] Setting up link handlers')
}

onMounted(() => {
  nextTick(() => {
    setupLinkHandler()
    if (viewerRef.value) {
      viewerRef.value.addEventListener('click', (e: MouseEvent) => {
        const anchor = (e.target as HTMLElement).closest('a')
        if (!anchor) return
        e.preventDefault()
        e.stopPropagation()
        console.log('[DocsViewer] Link clicked')
        const href = anchor.getAttribute('data-original-href') || anchor.getAttribute('href')
        console.log('[DocsViewer] Original href:', href)
        let docPath = href || ''
        if (href?.includes('.md')) {
          if (href.startsWith('./')) {
            docPath = href.slice(2)
          } else if (href.startsWith('../')) {
            docPath = href.slice(3)
          } else if (href.startsWith('/')) {
            docPath = href.slice(1)
          }
          if (!docPath.startsWith('docs/') && !docPath.startsWith('/')) {
            docPath = 'docs/' + docPath
          }
        }
        console.log('[DocsViewer] Navigating to:', docPath)
        emit('navigate', docPath)
      })
      const observer = new MutationObserver(() => {
        setupLinkHandler()
      })
      observer.observe(viewerRef.value, { childList: true, subtree: true })
    }
  })
})
</script>