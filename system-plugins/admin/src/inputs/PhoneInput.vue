<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputMask :placeholder="'Phone'" mask="(999) 999-9999" fluid class="text-sm" />
</template>
