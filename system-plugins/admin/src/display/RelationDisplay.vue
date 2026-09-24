<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { targetAppPath } from "@/utils/appHeaders";
import { relationId, relationLabel } from "@/utils/relations";
import { useAppContextStore } from "@/stores/appContext";

const props = defineProps<{
    value: string | Record<string, any> | null | undefined;
    relatedCollection?: string;
    relatedApp?: string;
    relatedField?: string;
    displayValue?: string | null;
    displayField?: string;
}>();

const router = useRouter(),
    appContext = useAppContextStore();

const currentVersion = computed(() => appContext.version ?? undefined),
    id = computed(() => relationId(props.value));

const label = computed(() =>
    relationLabel(props.value, {
        displayField: props.displayField,
        displayValue: props.displayValue,
    }),
);

function openRecord() {
    router.push(
        targetAppPath(
            props.relatedApp,
            currentVersion.value,
            "/detail/" + props.relatedCollection + "/" + id.value,
        ),
    );
}
</script>

<template>
    <span v-if="!id" class="text-gray-300">—</span>
    <a
        v-else
        class="text-blue-500 hover:text-blue-700 hover:underline font-medium cursor-pointer"
        :title="`View ${label} in ${relatedCollection}`"
        @click.stop="openRecord"
    >
        {{ label }}
    </a>
</template>
