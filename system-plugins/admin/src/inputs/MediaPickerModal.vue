<script setup lang="ts">
import { ref, watch } from 'vue'
import { useAlcedoClient } from '@/composables/useAlcedoClient'

const props = withDefaults(defineProps<{
  visible: boolean
  multiple?: boolean
}>(), {
  multiple: false,
})

const emit = defineEmits<{
  'update:visible': [value: boolean]
  select: [value: any]
}>()

interface MediaFile {
  id: string
  filename: string
  size_bytes: number
  mime_type: string
  created_at: string
}

const { client } = useAlcedoClient()

const files = ref<MediaFile[]>([])
const loading = ref(false)
const search = ref('')
const typeFilter = ref<string>('')
const selected = ref<any>(props.multiple ? [] : null)

const fileTypes = ['image', 'application', 'text', 'video', 'audio']

function close() {
  emit('update:visible', false)
}

async function fetchFiles() {
  loading.value = true
  try {
    const params: { limit: number; mime_type?: string } = { limit: 100 }
    if (typeFilter.value) params.mime_type = typeFilter.value + '/'
    const data = await client.files.list(params) as { data: MediaFile[] }
    files.value = data.data || []
  } catch {
    files.value = []
  } finally {
    loading.value = false
  }
}

function toggleSelect(file: MediaFile) {
  if (props.multiple) {
    const arr = selected.value as string[]
    const idx = arr.indexOf(file.id)
    if (idx >= 0) arr.splice(idx, 1)
    else arr.push(file.id)
  } else {
    selected.value = file.id
  }
}

function isSelected(id: string): boolean {
  if (props.multiple) return (selected.value as string[]).includes(id)
  return selected.value === id
}

function confirm() {
  emit('select', selected.value)
  close()
}

watch(() => props.visible, (v) => {
  if (v) {
    selected.value = props.multiple ? [] : null
    fetchFiles()
  }
})
</script>

<template>
  <Dialog
    :visible="visible"
    @update:visible="emit('update:visible', $event)"
    header="Media Library"
    :style="{ width: '50rem' }"
    :modal="true"
  >
    <div class="flex gap-2 mb-4">
      <InputText
        v-model="search"
        placeholder="Search files..."
        class="flex-1"
      />
      <Select
        v-model="typeFilter"
        :options="fileTypes"
        placeholder="All types"
        class="w-40"
        :showClear="true"
      />
    </div>

    <div v-if="loading" class="text-center py-8 text-gray-400">
      Loading...
    </div>

    <div v-else-if="files.length === 0" class="text-center py-8 text-gray-400">
      No files found
    </div>

    <div v-else class="grid grid-cols-4 gap-3 max-h-96 overflow-y-auto">
      <div
        v-for="file in files"
        :key="file.id"
        class="border rounded-lg p-2 cursor-pointer transition-colors"
        :class="isSelected(file.id) ? 'border-primary bg-primary/10' : 'border-gray-200 hover:border-gray-400'"
        @click="toggleSelect(file)"
      >
        <div class="aspect-square flex items-center justify-center bg-gray-50 rounded mb-1 overflow-hidden">
          <img
            v-if="file.mime_type.startsWith('image/')"
            :src="`/api/files/${file.id}/download`"
            class="w-full h-full object-cover"
          />
          <i v-else class="pi pi-file text-2xl text-gray-400"></i>
        </div>
        <div class="text-xs truncate">{{ file.filename }}</div>
      </div>
    </div>

    <template #footer>
      <Button label="Cancel" severity="secondary" @click="close" />
      <Button
        label="Select"
        :disabled="multiple ? (selected as string[]).length === 0 : !selected"
        @click="confirm"
      />
    </template>
  </Dialog>
</template>
