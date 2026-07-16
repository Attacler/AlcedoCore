<template>
  <span class="color-badge" :style="{ backgroundColor: bgColor, color: textColor }">{{ value }}</span>
</template>

<script setup>
import { computed } from 'vue'
const props = defineProps({ value: { type: [String, Number], default: '' } })
const colors = ['#dbeafe','#fef3c7','#d1fae5','#fce7f3','#e0e7ff','#fae8ff']
const bgColor = computed(() => colors[String(props.value).length % colors.length])
const textColor = computed(() => '#1f2937')
</script>

<style scoped>
.color-badge { display: inline-block; padding: 2px 10px; border-radius: 999px; font-size: 12px; font-weight: 500; }
</style>
