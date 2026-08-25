<script setup lang="ts">
import { ref, watch } from 'vue'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import Select from 'primevue/select'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import ToggleSwitch from 'primevue/toggleswitch'

const props = defineProps<{
  field: any
  collectionName: string
}>()

const { client } = useAlcedoClient()
const folders = ref<{ id: string; name: string }[]>([])

watch(() => props.field, (f) => {
  if (f) {
    f.options = f.options || {}
    if (f.options.folder_id === undefined) f.options.folder_id = null
    if (f.options.max_file_size === undefined) f.options.max_file_size = 10485760
    if (f.options.allowed_mime_types === undefined) f.options.allowed_mime_types = []
    loadFolders()
  }
}, { immediate: true })

async function loadFolders() {
  try {
    const data = (await client.files.folders.list()) as {
      data: { id: string; name: string }[]
    }
    folders.value = data?.data || []
  } catch {
    folders.value = []
  }
}
</script>

<template>
  <div class="space-y-4 pt-4 border-t border-gray-200">
    <h4 class="text-sm font-semibold text-gray-700">File Options</h4>

    <div>
      <label class="block text-xs font-medium text-gray-600 mb-1"
        >Upload Folder</label
      >
      <Select
        v-model="field.options.folder_id"
        :options="folders"
        option-label="name"
        option-value="id"
        show-clear
        placeholder="Root (no folder)"
        class="w-full"
        fluid
      />
      <p class="text-xs text-gray-400 mt-1">
        Uploaded files are saved into this media-library folder.
      </p>
    </div>

    <div>
      <label class="block text-xs font-medium text-gray-600 mb-1"
        >Allow Multiple</label
      >
      <ToggleSwitch v-model="field.options.multiple" />
      <p class="text-xs text-gray-400 mt-1">
        Allow uploading multiple files to this field
      </p>
    </div>

    <div>
      <label class="block text-xs font-medium text-gray-600 mb-1"
        >Max File Size (bytes)</label
      >
      <InputNumber
        v-model="field.options.max_file_size"
        :min="0"
        :step="1048576"
        class="w-full"
        fluid
      />
      <p class="text-xs text-gray-400 mt-1">
        Maximum file size in bytes. 0 = no limit. Default: 10MB (10485760)
      </p>
    </div>

    <div>
      <label class="block text-xs font-medium text-gray-600 mb-1"
        >Allowed MIME Types</label
      >
      <InputText
        :model-value="(field.options?.allowed_mime_types || []).join(', ')"
        @update:model-value="
          (val: any) => {
            field.options.allowed_mime_types = (val || '')
              .split(',')
              .map((s: string) => s.trim())
              .filter((s: string) => s.length > 0);
          }
        "
        placeholder="image/*, application/pdf"
        class="w-full"
        fluid
      />
      <p class="text-xs text-gray-400 mt-1">
        Leave empty to allow all types. Use glob patterns like image/*
      </p>
    </div>
  </div>
</template>
