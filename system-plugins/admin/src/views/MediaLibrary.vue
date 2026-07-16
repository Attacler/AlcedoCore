<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import { useToast } from '@/composables/useToast'
import { formatFileSize } from '@/utils/formatters'

interface MediaFile {
  id: string
  filename: string
  mime_type: string
  size_bytes: number
  alt_text: string | null
  created_at: string
  updated_at: string
  download_url: string
}

const { client } = useAlcedoClient()
const toast = useToast()

const files = ref<MediaFile[]>([])
const total = ref(0)
const loading = ref(false)
const error = ref<string | null>(null)

const page = ref(1)
const limit = 24
const search = ref('')
const typeFilter = ref('')
const showUploadZone = ref(false)

const selectedFile = ref<MediaFile | null>(null)
const showSidebar = ref(false)
const editAltText = ref('')
const savingAltText = ref(false)
const deleting = ref(false)
const showDeleteConfirm = ref(false)

const uploading = ref(false)
const uploadProgress = ref(0)

const filterOptions = [
  { label: 'All', value: '' },
  { label: 'Images', value: 'image/' },
  { label: 'Documents', value: 'application/' },
  { label: 'Other', value: 'other' },
]

const offset = computed(() => (page.value - 1) * limit)

let searchTimeout: ReturnType<typeof setTimeout> | null = null

function onSearchInput() {
  if (searchTimeout) clearTimeout(searchTimeout)
  searchTimeout = setTimeout(() => {
    page.value = 1
    fetchFiles()
  }, 300)
}

function onFilterChange(value: string) {
  typeFilter.value = value
  page.value = 1
  fetchFiles()
}

async function fetchFiles() {
  loading.value = true
  error.value = null
  try {
    const params: { limit: number; offset: number; search?: string; mime_type?: string } = {
      limit,
      offset: offset.value,
    }
    if (search.value) params.search = search.value
    if (typeFilter.value) {
      const mimeType = typeFilter.value === 'other' ? '' : typeFilter.value
      if (mimeType) params.mime_type = mimeType
    }
    const data = await client.files.list(params) as { data: MediaFile[]; total: number }
    files.value = data.data || []
    total.value = data.total || 0
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load files'
    files.value = []
    total.value = 0
  } finally {
    loading.value = false
  }
}

async function fetchFileDetail(id: string) {
  try {
    selectedFile.value = await client.files.get(id) as MediaFile
  } catch {
    toast.show('Failed to load file details', 'error')
  }
}

function openFileDetail(file: MediaFile) {
  selectedFile.value = file
  editAltText.value = file.alt_text || ''
  showSidebar.value = true
  fetchFileDetail(file.id)
}

function closeSidebar() {
  showSidebar.value = false
  selectedFile.value = null
}

async function saveAltText() {
  if (!selectedFile.value) return
  savingAltText.value = true
  try {
    await client.files.update(selectedFile.value.id, { alt_text: editAltText.value })
    toast.show('Alt text saved', 'success')
    selectedFile.value.alt_text = editAltText.value
    const idx = files.value.findIndex(f => f.id === selectedFile.value!.id)
    if (idx >= 0) files.value[idx].alt_text = editAltText.value
  } catch (e) {
    toast.show(`Failed to save: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    savingAltText.value = false
  }
}

function confirmDelete() {
  showDeleteConfirm.value = true
}

async function handleDelete() {
  if (!selectedFile.value) return
  deleting.value = true
  try {
    await client.files.delete(selectedFile.value.id)
    toast.show('File deleted', 'success')
    const id = selectedFile.value.id
    closeSidebar()
    files.value = files.value.filter(f => f.id !== id)
    total.value = Math.max(0, total.value - 1)
  } catch (e) {
    toast.show(`Failed to delete: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    deleting.value = false
    showDeleteConfirm.value = false
  }
}

function downloadFile(file: MediaFile) {
  window.open(file.download_url, '_blank')
}

function toggleUploadZone() {
  showUploadZone.value = !showUploadZone.value
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
  uploadProgress.value = 0
  const formData = new FormData()
  formData.append('file', file)

  const xhr = new XMLHttpRequest()
  xhr.upload.onprogress = (e) => {
    if (e.lengthComputable) uploadProgress.value = Math.round((e.loaded / e.total) * 100)
  }
  xhr.onload = () => {
    uploading.value = false
    if (xhr.status >= 200 && xhr.status < 300) {
      toast.show('File uploaded successfully', 'success')
      page.value = 1
      fetchFiles()
    } else {
      toast.show('Upload failed', 'error')
    }
  }
  xhr.onerror = () => {
    uploading.value = false
    toast.show('Upload failed', 'error')
  }
  xhr.open('POST', '/api/files/upload')
  xhr.send(formData)
}

function onPageChange(event: { page: number; rows: number }) {
  page.value = event.page + 1
  fetchFiles()
}

function isImage(mimeType: string): boolean {
  return mimeType.startsWith('image/')
}

function getFileIcon(mimeType: string): string {
  if (mimeType.startsWith('image/')) return 'pi pi-image'
  if (mimeType.startsWith('video/')) return 'pi pi-video'
  if (mimeType.startsWith('audio/')) return 'pi pi-volume'
  if (mimeType.startsWith('text/') || mimeType.includes('pdf') || mimeType.includes('document')) return 'pi pi-file'
  if (mimeType.includes('zip') || mimeType.includes('tar') || mimeType.includes('gzip')) return 'pi pi-file'
  return 'pi pi-file'
}

function formatDate(iso?: string): string {
  if (!iso) return '-'
  const d = new Date(iso)
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

onMounted(() => {
  fetchFiles()
})
</script>

<template>
  <div class="p-6">
    <!-- Header -->
    <div class="flex items-center justify-between mb-6">
      <h1 class="text-2xl font-bold text-gray-900">Media Library</h1>
      <Button
        icon="pi pi-upload"
        label="Upload"
        severity="primary"
        @click="toggleUploadZone"
      />
    </div>

    <!-- Collapsible Upload Zone -->
    <div
      v-if="showUploadZone"
      class="mb-6 border-2 border-dashed border-gray-300 rounded-xl p-8 text-center transition-colors"
      :class="{ 'border-primary bg-primary/5': uploading }"
      @drop.prevent="onDrop"
      @dragover.prevent
    >
      <div v-if="!uploading" class="space-y-3">
        <i class="pi pi-cloud-upload text-4xl text-gray-400 block"></i>
        <p class="text-gray-500">
          Drag & drop files here, or
          <label class="text-primary cursor-pointer hover:underline">
            browse
            <input type="file" class="hidden" @change="onFileSelected" />
          </label>
        </p>
      </div>
      <div v-else class="space-y-3">
        <i class="pi pi-spin pi-spinner text-3xl text-primary block"></i>
        <p class="text-gray-500">Uploading...</p>
        <ProgressBar :value="uploadProgress" class="max-w-md mx-auto" />
      </div>
    </div>

    <!-- Search & Filters -->
    <div class="flex flex-wrap items-center gap-3 mb-6">
      <span class="p-input-icon-left flex-1 min-w-[200px]">
        <i class="pi pi-search" />
        <InputText
          v-model="search"
          placeholder="Search files..."
          class="w-full"
          @input="onSearchInput"
        />
      </span>
      <div class="flex gap-2">
        <button
          v-for="opt in filterOptions"
          :key="opt.value"
          class="px-3 py-1.5 text-sm rounded-full border transition-colors cursor-pointer"
          :class="typeFilter === opt.value
            ? 'bg-primary text-white border-primary'
            : 'bg-white text-gray-600 border-gray-300 hover:border-gray-400'"
          @click="onFilterChange(opt.value)"
        >
          {{ opt.label }}
        </button>
      </div>
    </div>

    <!-- Loading State -->
    <div v-if="loading" class="text-center py-16 text-gray-400">
      <i class="pi pi-spin pi-spinner text-3xl block mb-3"></i>
      <p>Loading files...</p>
    </div>

    <!-- Error State -->
    <div v-else-if="error" class="mb-6">
      <Message severity="error" :closable="false">
        {{ error }}
      </Message>
      <Button label="Retry" severity="secondary" outlined @click="fetchFiles" class="mt-2" />
    </div>

    <!-- Empty State -->
    <div v-else-if="files.length === 0" class="text-center py-16">
      <Message severity="info" :closable="false">
        <template #icon>
          <i class="pi pi-image text-2xl mr-2" />
        </template>
        <span v-if="search || typeFilter">No files match your search criteria.</span>
        <span v-else>No files uploaded yet. Click "Upload" to get started.</span>
      </Message>
    </div>

    <!-- Grid View -->
    <div v-else class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4 mb-6">
      <div
        v-for="file in files"
        :key="file.id"
        class="bg-white rounded-lg border border-gray-200 overflow-hidden cursor-pointer hover:shadow-md transition-shadow"
        @click="openFileDetail(file)"
      >
        <!-- Thumbnail / Icon -->
        <div class="aspect-square flex items-center justify-center bg-gray-50 overflow-hidden">
          <img
            v-if="isImage(file.mime_type)"
            :src="`/api/files/${file.id}/download`"
            :alt="file.alt_text || file.filename"
            class="w-full h-full object-cover"
          />
          <i v-else :class="[getFileIcon(file.mime_type), 'text-3xl text-gray-400']" />
        </div>
        <!-- Info -->
        <div class="p-2 space-y-1">
          <div class="text-xs font-medium text-gray-800 truncate" :title="file.filename">
            {{ file.filename }}
          </div>
          <div class="flex items-center justify-between text-[10px] text-gray-500">
            <span>{{ formatFileSize(file.size_bytes) }}</span>
            <span>{{ formatDate(file.created_at) }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Pagination -->
    <div v-if="total > limit" class="flex justify-center">
      <Paginator
        :first="(page - 1) * limit"
        :rows="limit"
        :totalRecords="total"
        @page="onPageChange"
      />
    </div>

    <!-- File Detail Sidebar -->
    <Sidebar
      :visible="showSidebar"
      @update:visible="closeSidebar"
      header="File Details"
      position="right"
      :style="{ width: '28rem' }"
    >
      <div v-if="selectedFile" class="space-y-5">
        <!-- Preview -->
        <div class="rounded-lg overflow-hidden bg-gray-50 flex items-center justify-center max-h-48">
          <img
            v-if="isImage(selectedFile.mime_type)"
            :src="`/api/files/${selectedFile.id}/download`"
            :alt="selectedFile.alt_text || selectedFile.filename"
            class="w-full h-48 object-contain"
          />
          <i v-else :class="[getFileIcon(selectedFile.mime_type), 'text-5xl text-gray-400 py-8']" />
        </div>

        <!-- Filename -->
        <div>
          <label class="block text-xs font-semibold text-gray-500 uppercase mb-1">Filename</label>
          <p class="text-sm text-gray-800 break-all">{{ selectedFile.filename }}</p>
        </div>

        <!-- Alt Text -->
        <div>
          <label class="block text-xs font-semibold text-gray-500 uppercase mb-1">Alt Text</label>
          <div class="flex gap-2">
            <InputText
              v-model="editAltText"
              placeholder="Describe this file..."
              class="flex-1"
              :disabled="savingAltText"
            />
            <Button
              icon="pi pi-check"
              severity="primary"
              :loading="savingAltText"
              :disabled="editAltText === (selectedFile.alt_text || '')"
              @click="saveAltText"
            />
          </div>
        </div>

        <!-- Metadata -->
        <div>
          <label class="block text-xs font-semibold text-gray-500 uppercase mb-2">Metadata</label>
          <div class="space-y-1.5 text-sm text-gray-700">
            <div class="flex justify-between">
              <span class="text-gray-500">Size</span>
              <span>{{ formatFileSize(selectedFile.size_bytes) }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-gray-500">Type</span>
              <span>{{ selectedFile.mime_type }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-gray-500">Uploaded</span>
              <span>{{ formatDate(selectedFile.created_at) }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-gray-500">Updated</span>
              <span>{{ formatDate(selectedFile.updated_at) }}</span>
            </div>
          </div>
        </div>

        <!-- Actions -->
        <div class="flex gap-2 pt-2 border-t border-gray-200">
          <Button
            icon="pi pi-download"
            label="Download"
            severity="secondary"
            outlined
            class="flex-1"
            @click="downloadFile(selectedFile)"
          />
          <Button
            icon="pi pi-trash"
            label="Delete"
            severity="danger"
            outlined
            class="flex-1"
            @click="confirmDelete"
          />
        </div>
      </div>
    </Sidebar>

    <!-- Delete Confirmation Dialog -->
    <Dialog
      v-model:visible="showDeleteConfirm"
      header="Delete File"
      :modal="true"
      :style="{ width: '450px' }"
      :draggable="false"
    >
      <p class="text-gray-600 mb-4">
        Are you sure you want to delete "{{ selectedFile?.filename }}"? This cannot be undone.
      </p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showDeleteConfirm = false" />
        <Button label="Delete" severity="danger" :loading="deleting" @click="handleDelete" />
      </template>
    </Dialog>
  </div>
</template>
