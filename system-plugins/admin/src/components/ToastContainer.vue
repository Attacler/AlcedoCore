<script setup lang="ts">
import { useToast } from '@/composables/useToast'
const { toasts } = useToast()
</script>

<template>
  <div class="toast-container">
    <div
      v-for="toast in toasts"
      :key="toast.id"
      class="toast"
      :class="toast.type"
    >
      {{ toast.message }}
    </div>
  </div>
</template>

<style scoped>
.toast-container {
  position: fixed;
  top: 1rem;
  right: 1rem;
  z-index: 9999;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.toast {
  padding: 0.75rem 1rem;
  border-radius: 6px;
  color: white;
  font-size: 0.875rem;
  box-shadow: 0 4px 12px rgba(0,0,0,0.15);
  animation: slideIn 0.2s ease-out;
}
.toast.info { background: #3b82f6; }
.toast.success { background: #22c55e; }
.toast.warning { background: #f59e0b; }
.toast.error { background: #ef4444; }
@keyframes slideIn {
  from { transform: translateX(100%); opacity: 0; }
  to { transform: translateX(0); opacity: 1; }
}
</style>