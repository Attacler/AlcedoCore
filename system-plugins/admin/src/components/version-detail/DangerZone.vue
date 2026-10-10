<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import { useConfirm } from "primevue/useconfirm";
import Button from "primevue/button";

const PRODUCTION_VERSION = "production";

const props = defineProps<{ versionId: number; versionName: string }>();

const router = useRouter(),
    toast = useToast(),
    confirm = useConfirm(),
    { client } = useAlcedoClient();

// The production version is the main deploy target and cannot be deleted
// (the backend rejects it too), so the whole tab stays hidden for it.
const isProduction = computed(() => props.versionName === PRODUCTION_VERSION);

function confirmDelete() {
    confirm.require({
        message: `Delete version "${props.versionName}"? Every app on this version and all of its data will be removed. This cannot be undone.`,
        header: "Delete Version",
        icon: "pi pi-exclamation-triangle",
        rejectProps: {
            label: "Cancel",
            severity: "secondary",
            outlined: true,
        },
        acceptProps: { label: "Delete", severity: "danger" },
        accept: async () => {
            try {
                await client.versions.remove(props.versionId);
                toast.show(`Version "${props.versionName}" deleted`, "success");
                router.push("/versions");
            } catch (e) {
                toast.show(
                    "Failed to delete version: " +
                        (e instanceof Error ? e.message : e),
                    "error",
                );
            }
        },
    });
}
</script>

<template>
    <div
        v-if="!isProduction"
        class="bg-white rounded-lg shadow-sm border border-red-200 p-4"
    >
        <div class="flex items-center gap-2 mb-1">
            <span class="material-symbols-outlined text-lg text-red-600"
                >warning</span
            >
            <h3 class="font-medium text-gray-900">Delete this version</h3>
        </div>
        <p class="text-sm text-gray-600 mb-4">
            Removing
            <span class="font-medium text-gray-900">{{ versionName }}</span>
            deletes every app linked to it along with its data. This cannot be
            undone.
        </p>
        <Button
            label="Delete Version"
            icon="pi pi-trash"
            severity="danger"
            outlined
            @click="confirmDelete"
        />
    </div>
</template>
