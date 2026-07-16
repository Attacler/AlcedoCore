<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputText :placeholder="'Email'" type="email" fluid class="text-sm" />
</template>
