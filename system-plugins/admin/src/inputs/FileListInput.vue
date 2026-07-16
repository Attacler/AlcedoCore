<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { formatFileSize } from '@/utils/formatters'
import { useAlcedoClient } from '@/composables/useAlcedoClient'

const props = withDefaults(defineProps<{
  modelValue?: string[]
  field?: any
  invalid?: boolean | string
  readonly?: boolean
}>(), {
  modelValue: () => [],
})

const emit = defineEmits<{
  'update:modelValue': [value: string[]]
}>()

interface FileItem {
  id: string
  filename: string
  size_bytes: number
  mime_type: string
}

const { client } = useAlcedoClient()

const fileInput = ref<HTMLInputElement | null>(null)
const uploading = ref(false)
const progress = ref(0)
const items = ref<FileItem[]>([])
const showPicker = ref(false)
const dragIndex = ref<number | null>(null)

function isImage(mime: string) {
  return mime.startsWith('image/')
}

function browse() {
  fileInput.value?.click()
}

function onFileSelected(e: Event) {
  const target = e.target as HTMLInputElement
  if (target.files?.length) {
    startUpload(target.files[0])
    target.value = ''
  }
}

function onDrop(e: DragEvent) {
  if (e.dataTransfer?.files.length) {
    startUpload(e.dataTransfer.files[0])
  }
}

function startUpload(file: File) {
  uploading.value = true
  progress.value = 0
  uploadFile(file).catch(() => {
    uploading.value = false
  })
}

async function uploadFile(file: File) {
  const formData = new FormData()
  formData.append('file', file)

  return new Promise<void>((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable) progress.value = Math.round((e.loaded / e.total) * 100)
    }
    xhr.onload = () => {
      uploading.value = false
      if (xhr.status >= 200 && xhr.status < 300) {
        const result = JSON.parse(xhr.responseText)
        items.value.push({
          id: result.id,
          filename: file.name,
          size_bytes: file.size,
          mime_type: file.type,
        })
        emit('update:modelValue', items.value.map(i => i.id))
        resolve()
      } else {
        reject(new Error(xhr.statusText))
      }
    }
    xhr.onerror = () => {
      uploading.value = false
      reject(new Error('Upload failed'))
    }
    xhr.open('POST', '/api/files/upload')
    xhr.send(formData)
  })
}

function removeFile(index: number) {
  items.value.splice(index, 1)
  emit('update:modelValue', items.value.map(i => i.id))
}

function onPickFromLibrary(ids: string | string[]) {
  const idList = Array.isArray(ids) ? ids : [ids]
  for (const id of idList) {
    items.value.push({
      id,
      filename: '',
      size_bytes: 0,
      mime_type: 'application/octet-stream',
    })
  }
  emit('update:modelValue', items.value.map(i => i.id))
  showPicker.value = false
}

function onDragStart(index: number) {
  dragIndex.value = index
}

function onDragOver(e: DragEvent, index: number) {
  e.preventDefault()
  if (dragIndex.value === null || dragIndex.value === index) return
  const item = items.value.splice(dragIndex.value, 1)[0]
  items.value.splice(index, 0, item)
  dragIndex.value = index
}

function onDragEnd() {
  dragIndex.value = null
  emit('update:modelValue', items.value.map(i => i.id))
}

async function loadFileInfos() {
  for (const id of props.modelValue) {
    try {
      const info = await client.files.get(id) as FileItem
      items.value.push({
        id: info.id,
        filename: info.filename,
        size_bytes: info.size_bytes,
        mime_type: info.mime_type,
      })
    } catch {
      items.value.push({
        id,
        filename: id,
        size_bytes: 0,
        mime_type: 'application/octet-stream',
      })
    }
  }
}

onMounted(() => {
  if (props.modelValue?.length) {
    loadFileInfos()
  }
})
</script>

<template>
  <div class="file-list-input">
    <div class="upload-zone border-2 border-dashed border-gray-300 rounded-lg p-4 text-center cursor-pointer hover:border-primary transition-colors mb-2" @drop.prevent="onDrop" @dragover.prevent @click="browse">
      <i class="pi pi-upload text-xl text-gray-400 mb-1 block"></i>
      <p class="text-sm text-gray-500">
        Drag & drop a file here, or <a class="text-primary cursor-pointer">browse</a>
      </p>
      <input ref="fileInput" type="file" hidden @change="onFileSelected" />
    </div>

    <div v-if="uploading" class="progress-bar mb-2">
      <ProgressBar :value="progress" />
    </div>

    <div class="file-list space-y-1">
      <div
        v-for="(item, index) in items"
        :key="item.id"
        class="flex items-center gap-3 border rounded-lg p-2 cursor-grab"
        draggable="true"
        @dragstart="onDragStart(index)"
        @dragover="onDragOver($event, index)"
        @dragend="onDragEnd"
      >
        <i class="pi pi-bars text-gray-400 cursor-grab"></i>
        <img v-if="isImage(item.mime_type)" :src="`/api/files/${item.id}/download`" class="w-10 h-10 object-cover rounded" />
        <i v-else class="pi pi-file text-xl text-gray-400"></i>
        <div class="flex-1 min-w-0">
          <div class="text-sm truncate">{{ item.filename || item.id }}</div>
          <div v-if="item.size_bytes" class="text-xs text-gray-500">{{ formatFileSize(item.size_bytes) }}</div>
        </div>
        <Button icon="pi pi-trash" severity="danger" text @click="removeFile(index)" />
      </div>
    </div>

    <div v-if="items.length > 0" class="text-xs text-gray-400 mb-2">
      {{ items.length }} file(s)
    </div>

    <Button label="Pick from Media Library" icon="pi pi-folder" severity="secondary" text @click="showPicker = true" />

    <MediaPickerModal v-model:visible="showPicker" :multiple="true" @select="onPickFromLibrary" />
  </div>
</template>
