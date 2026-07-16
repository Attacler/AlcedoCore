<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/authStore'
import { useToast } from '@/composables/useToast'

const authStore = useAuthStore()
const router = useRouter()
const toast = useToast()
const forceLogoutLoading = ref(false)

const displayName = computed(() => {
  return authStore.user?.display_name || authStore.user?.email?.split('@')[0] || 'User'
})

async function handleLogout() {
  await authStore.logout()
  router.push('/login')
}

async function handleForceLogout() {
  forceLogoutLoading.value = true
  try {
    await fetch('/api/auth/logout', {
      method: 'POST',
      credentials: 'include',
    })
    authStore.user = null
    toast.show('All sessions terminated', 'success')
    router.push('/login')
  } catch {
    toast.show('Failed to terminate sessions', 'error')
  } finally {
    forceLogoutLoading.value = false
  }
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center gap-3 mb-6">
      <router-link to="/settings" class="material-symbols-outlined text-gray-400 hover:text-gray-600 transition-colors">
        arrow_back
      </router-link>
      <div>
        <div class="flex items-center gap-2">
          <span class="material-symbols-outlined text-gray-500">lock</span>
          <h1 class="text-2xl font-semibold text-gray-900">Session</h1>
        </div>
        <p class="text-sm text-gray-500 ml-8">Session management and security</p>
      </div>
    </div>

    <!-- Current Session Info -->
    <Card v-if="authStore.user">
      <template #title>
        <div class="flex items-center gap-2">
          <span class="material-symbols-outlined text-blue-500">person</span>
          <span>Current Session</span>
        </div>
      </template>
      <template #content>
        <div class="space-y-3">
          <div class="flex items-center justify-between py-2 border-b border-gray-100">
            <span class="text-sm text-gray-500">User</span>
            <span class="text-sm font-medium">{{ displayName }}</span>
          </div>
          <div class="flex items-center justify-between py-2 border-b border-gray-100">
            <span class="text-sm text-gray-500">Email</span>
            <span class="text-sm font-medium">{{ authStore.user.email }}</span>
          </div>
          <div class="flex items-center justify-between py-2 border-b border-gray-100">
            <span class="text-sm text-gray-500">Role</span>
            <span class="text-sm font-medium">{{ authStore.user.is_admin ? 'Admin' : 'User' }}</span>
          </div>
          <div class="flex items-center justify-between py-2">
            <span class="text-sm text-gray-500">Session</span>
            <span class="text-sm font-medium text-green-600 flex items-center gap-1">
              <span class="w-2 h-2 rounded-full bg-green-500 inline-block"></span>
              Active
            </span>
          </div>
        </div>
      </template>
    </Card>

    <!-- Actions -->
    <Card>
      <template #title>
        <div class="flex items-center gap-2">
          <span class="material-symbols-outlined text-orange-500">settings_power</span>
          <span>Actions</span>
        </div>
      </template>
      <template #content>
        <div class="space-y-4">
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm font-medium">Sign out</p>
              <p class="text-xs text-gray-500">End your current session</p>
            </div>
            <Button label="Logout" icon="pi pi-sign-out" severity="secondary" @click="handleLogout" />
          </div>
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm font-medium">Force logout all sessions</p>
              <p class="text-xs text-gray-500">Sign out from all devices and sessions</p>
            </div>
            <Button
              label="Force Logout"
              icon="pi pi-exclamation-triangle"
              severity="danger"
              :loading="forceLogoutLoading"
              @click="handleForceLogout"
            />
          </div>
        </div>
      </template>
    </Card>
  </div>
</template>