<script setup lang="ts">
defineProps({ value: { type: Number, default: 0 } });
</script>

<template>
    <div class="star-rating">
        <span
            v-for="n in 5"
            :key="n"
            class="star"
            :class="{ active: n <= value }"
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
    color: #d1d5db;
    transition: color 0.15s;
}
.star.active {
    color: #0b2ef5;
}
</style>
