<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import LogDetailPopup from "@/components/LogDetailPopup.vue";
import { formatLogTime } from "@/utils/formatters";
import { useActivityLogStore } from "@/stores/activityLogStore";

const props = defineProps<{
    collectionName: string;
    itemId: string;
}>();

const { client } = useAlcedoClient();

const activityStore = useActivityLogStore();

interface TimelineEntry {
    id: string;
    action: string;
    description: string | null;
    metadata?: Record<string, unknown> | null;
    diff?: Record<string, unknown> | null;
    created_at: string;
}

const entries = ref<TimelineEntry[]>([]),
    loading = ref(false),
    error = ref<string | null>(null),
    selectedEntry = ref<TimelineEntry | null>(null),
    showDetail = ref(false);

const detailTitle = computed(() => {
    if (!selectedEntry.value) return "Log Detail";
    return `${activityStore.formatActionLabel(selectedEntry.value.action)} — ${formatLogTime(selectedEntry.value.created_at)}`;
});

function openDetail(entry: TimelineEntry) {
    selectedEntry.value = entry;
    showDetail.value = true;
}

async function fetchTimeline() {
    if (!props.collectionName || !props.itemId) return;

    loading.value = true;
    error.value = null;

    try {
        const params = new URLSearchParams();
        params.set("target", props.collectionName);
        params.set("item_id", props.itemId);
        params.set("limit", "50");
        params.set("offset", "0");

        const response = (await client.activityLogs.listCollections(
            params,
        )) as {
            data: TimelineEntry[];
            total: number;
        };
        entries.value = response.data || [];
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load timeline";
    } finally {
        loading.value = false;
    }
}

onMounted(() => {
    fetchTimeline();
});
</script>

<template>
    <div class="activity-timeline">
        <h3
            class="text-sm font-semibold text-gray-700 uppercase tracking-wide mb-4 flex items-center gap-2"
        >
            <span class="pi pi-clock text-gray-400"></span>
            Activity
        </h3>

        <div v-if="loading" class="flex items-center justify-center py-12">
            <span class="pi pi-spin pi-spinner text-2xl text-gray-400"></span>
        </div>

        <div v-else-if="error" class="text-center py-8">
            <p class="text-sm text-red-600 mb-3">{{ error }}</p>
            <Button
                label="Retry"
                severity="danger"
                size="small"
                @click="fetchTimeline"
            />
        </div>

        <div
            v-else-if="entries.length === 0"
            class="text-center py-12 text-gray-400"
        >
            <span class="pi pi-history text-3xl block mb-2"></span>
            <p class="text-sm">No changes recorded for this item</p>
        </div>

        <div v-else class="space-y-0">
            <div
                v-for="(entry, idx) in entries"
                :key="entry.id"
                class="relative pl-6 pb-5 cursor-pointer hover:bg-gray-50 rounded transition-colors"
                :class="{
                    'border-l-2 border-gray-200': idx < entries.length - 1,
                }"
                @click="openDetail(entry)"
            >
                <div
                    class="absolute left-0 top-1 w-3 h-3 rounded-full border-2 -translate-x-[7px]"
                    :class="activityStore.dotClass(entry.action)"
                ></div>
                <div class="flex items-center gap-2 mb-1">
                    <Tag
                        :value="activityStore.formatActionLabel(entry.action)"
                        :severity="activityStore.getSeverity(entry.action)"
                        class="text-xs"
                    />
                </div>
                <p v-if="entry.description" class="text-xs text-gray-600 mb-1">
                    {{ entry.description }}
                </p>
                <p class="text-xs text-gray-400">
                    {{ formatLogTime(entry.created_at) }}
                </p>
            </div>
        </div>

        <LogDetailPopup
            v-model:visible="showDetail"
            :title="detailTitle"
            :description="selectedEntry?.description"
            :metadata="
                selectedEntry?.metadata as Record<string, unknown> | null
            "
            :diff="selectedEntry?.diff as Record<string, unknown> | null"
        />
    </div>
</template>
