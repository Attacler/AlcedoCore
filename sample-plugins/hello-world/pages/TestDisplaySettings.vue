<script setup lang="ts">
defineProps({ value: { type: Number, default: 0 } });
</script>

<template>
    <div class="test-class"><input type="number" /> asd</div>
</template>

<style scoped>
.test-class {
    color: rgb(87, 165, 42);
}
</style>
