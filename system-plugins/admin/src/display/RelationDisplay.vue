<script setup lang="ts">
import { useRouter } from "vue-router";

const props = defineProps<{
    value: string | null | undefined;
    relatedCollection?: string;
    relatedField?: string;
    displayValue?: string | null;
}>();

const router = useRouter();

function openRecord() {
    router.push("/detail/" + props.relatedCollection + "/" + props.value);
}
</script>

<template>
    <span v-if="!value" class="text-gray-300">—</span>
    <a
        v-else
        class="text-blue-500 hover:text-blue-700 hover:underline font-medium cursor-pointer"
        :title="`View ${displayValue || value} in ${relatedCollection}`"
        @click.stop="openRecord"
    >
        {{ displayValue || value }}
    </a>
</template>
