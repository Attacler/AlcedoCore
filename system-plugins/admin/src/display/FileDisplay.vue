<script setup lang="ts">
import { computed } from 'vue'
import { formatFileSize } from '@/utils/formatters'

interface FileMeta {
  id: string
  filename: string
  size_bytes: number
  mime_type: string
}

const props = defineProps<{
  value: FileMeta | null | undefined
}>()

const isImage = computed(() => {
  return props.value?.mime_type.startsWith('image/') ?? false
})

const thumbnailSrc = computed(() => {
  if (!props.value) return ''
  return `/api/files/${props.value.id}/download`
})

function download() {
  if (!props.value) return
  window.open(`/api/files/${props.value.id}/download`, '_blank')
}
</script>

<template>
  <div v-if="value" class="file-display flex items-center gap-3 p-2 border rounded-lg">
    <img
      v-if="isImage"
      :src="thumbnailSrc"
      class="w-12 h-12 object-cover rounded"
    />
    <i v-else class="pi pi-file text-2xl text-gray-400"></i>
    <div class="flex-1 min-w-0">
      <div class="text-sm font-medium truncate">{{ value.filename }}</div>
      <div class="text-xs text-gray-500">
        {{ formatFileSize(value.size_bytes) }} &middot; {{ value.mime_type }}
      </div>
    </div>
    <Button icon="pi pi-download" text severity="secondary" @click="download" />
  </div>
</template>
