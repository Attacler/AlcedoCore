<template>
  <Dialog
    :visible="visible"
    :header="header"
    :modal="true"
    :style="{ width: '450px' }"
    :draggable="false"
    @update:visible="onUpdateVisible"
  >
    <p class="text-gray-600">{{ message }}</p>
    <template #footer>
      <Button
        label="Cancel"
        severity="secondary"
        outlined
        @click="$emit('cancel')"
      />
      <Button
        :label="confirmLabel"
        severity="danger"
        :loading="loading"
        @click="$emit('confirm')"
      />
    </template>
  </Dialog>
</template>

<script setup lang="ts">
defineProps<{
  visible: boolean
  header: string
  message: string
  loading?: boolean
  confirmLabel?: string
}>()

const emit = defineEmits<{
  confirm: []
  cancel: []
}>()

function onUpdateVisible(val: boolean) {
  if (!val) emit('cancel')
}
</script>
