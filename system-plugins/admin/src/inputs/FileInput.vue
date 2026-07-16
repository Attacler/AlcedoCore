<script setup lang="ts">
import { ref, computed } from 'vue'
import { formatFileSize } from '@/utils/formatters'
import { useAlcedoClient } from '@/composables/useAlcedoClient'

const props = withDefaults(defineProps<{
  modelValue?: string | null
  field?: any
  invalid?: boolean | string
  readonly?: boolean
}>(), {
  modelValue: null,
})

const emit = defineEmits<{
  'update:modelValue': [value: string | null]
}>()

const { client } = useAlcedoClient()

const fileInput = ref<HTMLInputElement | null>(null)
const uploading = ref(false)
const progress = ref(0)
const showPicker = ref(false)

interface FileInfo {
  id: string
  filename: string
  size_bytes: number
  mime_type: string
}

const currentFile = ref<FileInfo | null>(null)

const isImage = computed(() => {
  return currentFile.value?.mime_type.startsWith('image/') ?? false
})

const previewUrl = computed(() => {
  if (!currentFile.value) return ''
  return `/api/files/${currentFile.value.id}/download`
})

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
  progress.value = 0

  return new Promise<void>((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable) progress.value = Math.round((e.loaded / e.total) * 100)
    }
    xhr.onload = () => {
      uploading.value = false
      if (xhr.status >= 200 && xhr.status < 300) {
        const result = JSON.parse(xhr.responseText)
        currentFile.value = {
          id: result.id,
          filename: file.name,
          size_bytes: file.size,
          mime_type: file.type,
        }
        emit('update:modelValue', result.id)
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

function removeFile() {
  currentFile.value = null
  emit('update:modelValue', null)
}

function onPickFromLibrary(id: string) {
  if (id) {
    currentFile.value = null
    emit('update:modelValue', id)
  }
  showPicker.value = false
}

async function loadFileInfo(id: string) {
  try {
    const data = await client.files.get(id) as FileInfo
    currentFile.value = data
  } catch {
    // file info not available
  }
}

if (props.modelValue) {
  loadFileInfo(props.modelValue)
}
</script>

<template>
  <div class="file-input">
    <div v-if="!currentFile && !uploading" class="upload-zone border-2 border-dashed border-gray-300 rounded-lg p-6 text-center cursor-pointer hover:border-primary transition-colors" @drop.prevent="onDrop" @dragover.prevent @click="browse">
      <i class="pi pi-upload text-2xl text-gray-400 mb-2 block"></i>
      <p class="text-sm text-gray-500">
        Drag & drop a file here, or <a class="text-primary cursor-pointer">browse</a>
      </p>
      <input ref="fileInput" type="file" hidden @change="onFileSelected" />
    </div>

    <div v-if="uploading" class="progress-bar my-2">
      <ProgressBar :value="progress" />
    </div>

    <div v-if="currentFile" class="file-preview flex items-center gap-3 border rounded-lg p-3">
      <img v-if="isImage" :src="previewUrl" class="w-12 h-12 object-cover rounded" />
      <i v-else class="pi pi-file text-2xl text-gray-400"></i>
      <div class="flex-1 min-w-0">
        <div class="text-sm font-medium truncate">{{ currentFile.filename }}</div>
        <div class="text-xs text-gray-500">{{ formatFileSize(currentFile.size_bytes) }}</div>
      </div>
      <Button icon="pi pi-trash" severity="danger" text @click="removeFile" />
    </div>

    <div v-if="!readonly" class="mt-2">
      <Button label="Pick from Media Library" icon="pi pi-folder" severity="secondary" text @click="showPicker = true" />
    </div>

    <MediaPickerModal v-model:visible="showPicker" @select="onPickFromLibrary" />
  </div>
</template>
