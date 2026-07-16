<script setup lang="ts">
import { ref, watch } from 'vue'
import type { Component } from 'vue'
import type { FieldDefinition } from '@/stores/collections'
import Drawer from 'primevue/drawer'

const props = defineProps<{
  visible: boolean
  settingsComponent: Component | null
  collectionName: string
  fields: FieldDefinition[]
  systemFields: string[]
  settings: Record<string, any>
}>()

const emit = defineEmits<{
  'update:visible': [value: boolean]
  'settings-change': [key: string, value: any]
}>()

const visible = ref(props.visible)

watch(() => props.visible, (val) => {
  visible.value = val
})

watch(visible, (val) => {
  emit('update:visible', val)
})

function onClose() {
  emit('update:visible', false)
}

function onSettingsChange(key: string, value: any) {
  emit('settings-change', key, value)
}
</script>

<template>
  <Drawer
    v-model:visible="visible"
    header="View Settings"
    position="right"
    class="w-full max-w-lg"
    @hide="onClose"
  >
    <component
      :is="settingsComponent"
      v-if="settingsComponent"
      :fields="fields"
      :system-fields="systemFields"
      :settings="settings"
      :collection-name="collectionName"
      :on-change="onSettingsChange"
    />
  </Drawer>
</template>