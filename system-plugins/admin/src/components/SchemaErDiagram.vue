<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import type { PluginSchemaResponse } from 'alcedo-sdk'
import { schemaToMermaid } from '../utils/schemaToMermaid'
import { initMermaid } from '../utils/mermaid'

const props = defineProps<{
  schema: PluginSchemaResponse
}>()

const containerRef = ref<HTMLDivElement | null>(null)

async function render() {
  if (!containerRef.value) return

  const mermaidCode = schemaToMermaid(props.schema)
  if (!mermaidCode) {
    containerRef.value.innerHTML = ''
    return
  }

  const { default: mermaid } = await initMermaid()

  containerRef.value.innerHTML = ''
  const { svg } = await mermaid.render('erDiagram', mermaidCode)
  containerRef.value.innerHTML = svg
}

onMounted(render)

watch(() => props.schema, render, { deep: true })
</script>

<template>
  <div ref="containerRef" class="schema-erd-diagram"></div>
</template>