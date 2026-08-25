<script setup lang="ts">
defineProps({ modelValue: { type: Number, default: 0 } });
defineEmits(["update:modelValue"]);
</script>

<template>
    <div class="star-rating">
        <span
            v-for="n in 5"
            :key="n"
            class="star"
            :class="{ active: n <= modelValue }"
            @click="$emit('update:modelValue', n)"
        >
            ★
        </span>
    </div>
</template>

<style scoped>
.star-rating {
    display: flex;
    gap: 4px;
}
.star {
    font-size: 24px;
    cursor: pointer;
    color: #d1d5db;
    transition: color 0.15s;
}
.star.active {
    color: #f59e0b;
}
.star:hover {
    color: #fbbf24;
}
</style>
