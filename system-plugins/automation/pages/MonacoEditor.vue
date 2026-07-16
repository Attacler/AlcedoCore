<template>
<div ref="editorContainer" style="width: 100%; flex: 1; min-height: 0;"></div>
<textarea v-if="showFallback" v-model="fallbackCode" class="fallback-editor" @input="onFallbackInput"></textarea>
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch } from 'vue'

const props = defineProps<{
  modelValue: string
  language?: string
  readOnly?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const editorContainer = ref<HTMLDivElement>()
const showFallback = ref(false)
const fallbackCode = ref('')
let editor: any = null

const SDK_TYPES = `
declare namespace alcedocore {
  var items: {
    get(collection: string, id: string): Promise<Record<string, any>>
  }
  var kv: {
    get(key: string): Promise<any>
    set(key: string, value: any, ttl?: number): Promise<any>
    delete(key: string): Promise<any>
  }
  var db: {
    query(sql: string, params?: string[]): Promise<any>
  }
  function health(): Promise<any>
}

declare var event: {
  type: 'ItemCreated' | 'ItemUpdated' | 'ItemDeleted'
  data: {
    collection_name: string
    item_id: string
    old_values?: Record<string, any>
    new_values?: Record<string, any>
    diff?: Record<string, { old: any; new: any }>
    values?: Record<string, any>
  }
}
`

function onFallbackInput(e: any) {
  emit('update:modelValue', e.target.value)
}

watch(() => props.modelValue, (newVal) => {
  if (editor && newVal !== editor.getValue()) {
    editor.setValue(newVal)
  }
  if (showFallback.value && newVal !== fallbackCode.value) {
    fallbackCode.value = newVal
  }
})

onMounted(async () => {
  fallbackCode.value = props.modelValue
  try {
    const script = document.createElement('script')
    script.src = 'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.0/min/vs/loader.js'
    script.onload = () => {
      if (!editorContainer.value) return
      ;(window as any).require.config({
        paths: { vs: 'https://cdn.jsdelivr.net/npm/monaco-editor@0.52.0/min/vs' }
      })
      ;(window as any).require(['vs/editor/editor.main'], () => {
        if (!editorContainer.value) return
        try {
          const monaco = (window as any).monaco
          // Register SDK types for autocomplete
          monaco.languages.typescript.javascriptDefaults.addExtraLib(SDK_TYPES, 'alcedocore.d.ts')
          monaco.languages.typescript.typescriptDefaults.addExtraLib(SDK_TYPES, 'alcedocore.d.ts')

          editor = monaco.editor.create(editorContainer.value, {
            value: props.modelValue,
            language: props.language || 'javascript',
            theme: 'vs-dark',
            readOnly: props.readOnly || false,
            minimap: { enabled: false },
            automaticLayout: true,
            fontSize: 14,
            tabSize: 2,
          })
          editor.onDidChangeModelContent(() => {
            emit('update:modelValue', editor.getValue())
          })
        } catch (e) {
          console.error('Monaco create error:', e)
          showFallback.value = true
        }
      })
    }
    script.onerror = () => {
      console.warn('Monaco CDN failed, using fallback textarea')
      showFallback.value = true
    }
    document.head.appendChild(script)
  } catch (e) {
    console.error('Monaco load error:', e)
    showFallback.value = true
  }
})

onBeforeUnmount(() => {
  if (editor) editor.dispose()
})
</script>

<style scoped>
.fallback-editor {
  width: 100%;
  height: 100%;
  min-height: 400px;
  font-family: 'SF Mono', Monaco, 'Cascadia Code', monospace;
  font-size: 14px;
  padding: 12px;
  border: 1px solid #ccc;
  border-radius: 4px;
  resize: none;
  background: #1e1e1e;
  color: #d4d4d4;
  tab-size: 2;
}
</style>
