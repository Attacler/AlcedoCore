<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
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

interface FileFolder {
  id: string
  name: string
  parent_id: string | null
  created_by: string | null
  created_at: string
  updated_at: string
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

const folders = ref<FileFolder[]>([])
const currentFolderId = ref<string | null>(null)
const folderPath = ref<FileFolder[]>([])
const showCreateFolderDialog = ref(false)
const newFolderName = ref('')
const creatingFolder = ref(false)
const showDeleteFolderDialog = ref(false)
const folderToDelete = ref<FileFolder | null>(null)
const deletingFolder = ref(false)
const showRenameFolderDialog = ref(false)
const folderToRename = ref<FileFolder | null>(null)
const renameFolderName = ref('')
const renamingFolder = ref(false)

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
    fetchFiles(currentFolderId.value)
  }, 300)
}

function onFilterChange(value: string) {
  typeFilter.value = value
  page.value = 1
  fetchFiles(currentFolderId.value)
}

async function fetchFiles(folderId?: string | null) {
  loading.value = true
  error.value = null
  try {
    const params: { limit: number; offset: number; search?: string; mime_type?: string; folder_id?: string } = {
      limit,
      offset: offset.value,
    }
    if (search.value) params.search = search.value
    if (typeFilter.value) {
      const mimeType = typeFilter.value === 'other' ? '' : typeFilter.value
      if (mimeType) params.mime_type = mimeType
    }
    if (folderId === null) {
      params.folder_id = ''
    } else if (folderId !== undefined) {
      params.folder_id = folderId
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
  if (currentFolderId.value) {
    formData.append('folder_id', currentFolderId.value)
  }

  const xhr = new XMLHttpRequest()
  xhr.upload.onprogress = (e) => {
    if (e.lengthComputable) uploadProgress.value = Math.round((e.loaded / e.total) * 100)
  }
  xhr.onload = () => {
    uploading.value = false
    if (xhr.status >= 200 && xhr.status < 300) {
      toast.show('File uploaded successfully', 'success')
      page.value = 1
      fetchFiles(currentFolderId.value)
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
  fetchFiles(currentFolderId.value)
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

async function fetchFolders(parentId?: string) {
  try {
    const data = await client.files.folders.list(parentId) as { data: FileFolder[] }
    return data.data || []
  } catch {
    return []
  }
}

async function loadCurrentFolders() {
  folders.value = await fetchFolders(currentFolderId.value || undefined)
}

async function buildFolderPath(folderId: string | null): Promise<FileFolder[]> {
  const path: FileFolder[] = []
  let current = folderId
  while (current) {
    try {
      const folder = await client.files.folders.get(current) as FileFolder
      path.unshift(folder)
      current = folder.parent_id
    } catch {
      break
    }
  }
  return path
}

function navigateToFolder(folder: FileFolder | null) {
  currentFolderId.value = folder ? folder.id : null
  page.value = 1
  loadCurrentFolders()
  fetchFiles(currentFolderId.value)
  buildFolderPath(currentFolderId.value).then(p => folderPath.value = p)
}

function navigateToRoot() {
  currentFolderId.value = null
  page.value = 1
  loadCurrentFolders()
  fetchFiles(currentFolderId.value)
  folderPath.value = []
}

function navigateToParent() {
  if (folderPath.value.length > 0) {
    const parent = folderPath.value[folderPath.value.length - 1].parent_id
    if (parent) {
      navigateToFolder({ id: parent } as FileFolder)
    } else {
      navigateToRoot()
    }
  }
}

async function createFolder() {
  if (!newFolderName.value.trim()) return
  creatingFolder.value = true
  try {
    await client.files.folders.create(newFolderName.value.trim(), currentFolderId.value || undefined)
    toast.show('Folder created', 'success')
    showCreateFolderDialog.value = false
    newFolderName.value = ''
    await loadCurrentFolders()
  } catch (e) {
    toast.show(`Failed to create folder: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    creatingFolder.value = false
  }
}

function confirmDeleteFolder(folder: FileFolder) {
  folderToDelete.value = folder
  showDeleteFolderDialog.value = true
}

async function handleDeleteFolder() {
  if (!folderToDelete.value) return
  deletingFolder.value = true
  try {
    await client.files.folders.delete(folderToDelete.value.id, true)
    toast.show('Folder deleted', 'success')
    showDeleteFolderDialog.value = false
    folderToDelete.value = null
    await loadCurrentFolders()
    if (currentFolderId.value) {
      fetchFiles(currentFolderId.value)
    }
  } catch (e) {
    toast.show(`Failed to delete folder: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    deletingFolder.value = false
  }
}

function confirmRenameFolder(folder: FileFolder) {
  folderToRename.value = folder
  renameFolderName.value = folder.name
  showRenameFolderDialog.value = true
}

async function handleRenameFolder() {
  if (!folderToRename.value || !renameFolderName.value.trim()) return
  renamingFolder.value = true
  try {
    await client.files.folders.update(folderToRename.value.id, { name: renameFolderName.value.trim() })
    toast.show('Folder renamed', 'success')
    showRenameFolderDialog.value = false
    folderToRename.value = null
    await loadCurrentFolders()
    await buildFolderPath(currentFolderId.value).then(p => folderPath.value = p)
  } catch (e) {
    toast.show(`Failed to rename folder: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    renamingFolder.value = false
  }
}

onMounted(() => {
  loadCurrentFolders()
  fetchFiles(currentFolderId.value)
})
</script>

<template>
  <div class="p-6">
    <div class="flex gap-6">
      <!-- Left sidebar - Folder Tree -->
      <div class="w-72 flex-shrink-0">
        <div class="bg-white rounded-xl border border-gray-200 overflow-hidden">
          <div class="p-3 border-b border-gray-100">
            <Button
              icon="pi pi-folder-plus"
              label="New Folder"
              severity="secondary"
              outlined
              size="small"
              class="w-full"
              @click="showCreateFolderDialog = true"
            />
          </div>
          <div class="p-2">
            <!-- Root (All Files) -->
            <div
              class="flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer text-sm transition-colors"
              :class="currentFolderId === null ? 'bg-primary/10 text-primary font-medium' : 'hover:bg-gray-100 text-gray-700'"
              @click="navigateToRoot()"
            >
              <i class="pi pi-inbox text-base"></i>
              <span>All Files</span>
            </div>

            <!-- Folder list -->
            <div v-if="folders.length > 0" class="mt-1 space-y-0.5">
              <div
                v-for="folder in folders"
                :key="folder.id"
                class="group flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer text-sm transition-colors"
                :class="currentFolderId === folder.id ? 'bg-primary/10 text-primary font-medium' : 'hover:bg-gray-100 text-gray-700'"
                @click="navigateToFolder(folder)"
              >
                <i class="pi pi-folder text-base"></i>
                <span class="flex-1 truncate">{{ folder.name }}</span>
                <div class="hidden group-hover:flex items-center gap-1">
                  <Button
                    icon="pi pi-pencil"
                    severity="secondary"
                    text
                    rounded
                    size="small"
                    @click.stop="confirmRenameFolder(folder)"
                  />
                  <Button
                    icon="pi pi-trash"
                    severity="danger"
                    text
                    rounded
                    size="small"
                    @click.stop="confirmDeleteFolder(folder)"
                  />
                </div>
              </div>
            </div>

            <!-- Empty folder state -->
            <div v-if="folders.length === 0 && currentFolderId !== null" class="px-3 py-4 text-center text-xs text-gray-400">
              This folder is empty
            </div>
          </div>
        </div>
      </div>

      <!-- Right pane - File grid -->
      <div class="flex-1 min-w-0">
        <!-- Breadcrumb -->
        <div class="flex items-center gap-2 mb-4 text-sm text-gray-500">
          <button
            class="hover:text-primary transition-colors cursor-pointer"
            @click="navigateToRoot()"
          >
            <i class="pi pi-home"></i>
          </button>
          <template v-for="(f, i) in folderPath" :key="f.id">
            <i class="pi pi-chevron-right text-xs"></i>
            <button
              class="hover:text-primary transition-colors cursor-pointer"
              :class="i === folderPath.length - 1 ? 'text-gray-900 font-medium' : ''"
              @click="navigateToFolder(f)"
            >
              {{ f.name }}
            </button>
          </template>
          <div class="flex-1"></div>
          <!-- Upload button -->
          <Button
            icon="pi pi-upload"
            label="Upload"
            severity="primary"
            size="small"
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
          <Button label="Retry" severity="secondary" outlined @click="fetchFiles(currentFolderId.value)" class="mt-2" />
        </div>

        <!-- Empty State -->
        <div v-else-if="files.length === 0" class="text-center py-16">
          <Message severity="info" :closable="false">
            <template #icon>
              <i class="pi pi-image text-2xl mr-2" />
            </template>
            <span v-if="search || typeFilter">No files match your search criteria.</span>
            <span v-else-if="currentFolderId">This folder has no files. Upload one to get started.</span>
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
      </div>
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

    <!-- Create Folder Dialog -->
    <Dialog
      v-model:visible="showCreateFolderDialog"
      header="Create Folder"
      :modal="true"
      :style="{ width: '400px' }"
      :draggable="false"
    >
      <div class="space-y-4">
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Folder Name</label>
          <InputText
            v-model="newFolderName"
            placeholder="Enter folder name"
            class="w-full"
            @keyup.enter="createFolder"
            autofocus
          />
        </div>
        <p v-if="folderPath.length > 0" class="text-xs text-gray-400">
          Location: Root / {{ folderPath.map(f => f.name).join(' / ') }}
        </p>
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showCreateFolderDialog = false" />
        <Button
          label="Create"
          severity="primary"
          :loading="creatingFolder"
          :disabled="!newFolderName.trim()"
          @click="createFolder"
        />
      </template>
    </Dialog>

    <!-- Delete Folder Dialog -->
    <Dialog
      v-model:visible="showDeleteFolderDialog"
      header="Delete Folder"
      :modal="true"
      :style="{ width: '450px' }"
      :draggable="false"
    >
      <p class="text-gray-600 mb-4">
        Are you sure you want to delete "{{ folderToDelete?.name }}"? This will permanently delete the folder and all its contents.
      </p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showDeleteFolderDialog = false" />
        <Button label="Delete" severity="danger" :loading="deletingFolder" @click="handleDeleteFolder" />
      </template>
    </Dialog>

    <!-- Rename Folder Dialog -->
    <Dialog
      v-model:visible="showRenameFolderDialog"
      header="Rename Folder"
      :modal="true"
      :style="{ width: '400px' }"
      :draggable="false"
    >
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Folder Name</label>
        <InputText
          v-model="renameFolderName"
          placeholder="Enter new name"
          class="w-full"
          @keyup.enter="handleRenameFolder"
          autofocus
        />
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showRenameFolderDialog = false" />
        <Button
          label="Save"
          severity="primary"
          :loading="renamingFolder"
          :disabled="!renameFolderName.trim() || renameFolderName === folderToRename?.name"
          @click="handleRenameFolder"
        />
      </template>
    </Dialog>
  </div>
</template>
