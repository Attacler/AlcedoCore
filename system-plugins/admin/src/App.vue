<script setup lang="ts">
import { ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import AppLayout from '@/components/AppLayout.vue'
import RelationalDrawer from '@/components/RelationalDrawer.vue'
import { usePluginsStore } from '@/stores/plugins'
import { useAuthStore } from '@/stores/authStore'

const router = useRouter()
const isLoginPage = ref(false)

const authStore = useAuthStore()
const store = usePluginsStore()

router.isReady().then(() => {
  isLoginPage.value = router.currentRoute.value.name === 'Login'

  router.afterEach((to) => {
    isLoginPage.value = to.name === 'Login'
  })
})

watch(() => authStore.user, (user) => {
  if (user) {
    store.fetchPlugins()
  }
}, { immediate: true })
</script>

<template>
  <template v-if="isLoginPage">
    <router-view />
  </template>
  <template v-else>
    <AppLayout />
  </template>
  <RelationalDrawer />
</template>