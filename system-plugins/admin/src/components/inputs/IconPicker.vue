<script lang="ts" setup>
import { computed, ref } from "vue";

const showIconPicker = ref(false),
    iconSearchQuery = ref("");

const currentIcon = defineModel();

const COMMON_ICONS = [
    "dashboard",
    "home",
    "folder",
    "extension",
    "settings",
    "list",
    "grid_view",
    "table",
    "view_list",
    "view_module",
    "search",
    "filter_alt",
    "sort",
    "add",
    "delete",
    "edit",
    "save",
    "close",
    "menu",
    "more_vert",
    "more_horiz",
    "people",
    "person",
    "star",
    "favorite",
    "bookmark",
    "share",
    "link",
    "open_in_new",
    "email",
    "notifications",
    "calendar_month",
    "schedule",
    "check_circle",
    "info",
    "warning",
    "error",
    "cloud",
    "download",
    "upload",
    "refresh",
    "sync",
    "lock",
    "visibility",
    "visibility_off",
    "palette",
    "tune",
    "code",
    "description",
    "file_copy",
    "history",
    "help",
    "question_answer",
    "bar_chart",
    "trending_up",
    "analytics",
    "assessment",
    "account_balance",
    "shopping_cart",
    "language",
    "rocket",
    "science",
    "psychology",
    "arrow_upward",
    "arrow_downward",
    "arrow_back",
    "arrow_forward",
    "drag_indicator",
    "reorder",
    "label",
    "category",
    "map",
    "location_on",
];

const filteredIcons = computed(() => {
    const q = iconSearchQuery.value.trim().toLowerCase();
    if (!q) return COMMON_ICONS;
    return COMMON_ICONS.filter((name) => name.includes(q));
});

function selectIcon(iconName: string) {
    currentIcon.value = iconName;
    showIconPicker.value = false;
}

function openIconPicker() {
    iconSearchQuery.value = "";
    showIconPicker.value = true;
}
</script>

<template>
    <div class="flex gap-2 items-center">
        <div
            class="w-10 h-10 flex items-center justify-center bg-gray-100 rounded-lg border border-gray-200"
        >
            <span class="material-symbols-outlined text-xl text-gray-600">{{
                currentIcon
            }}</span>
        </div>
        <Button
            label="Change Icon"
            severity="secondary"
            outlined
            @click="openIconPicker"
        />
        <Button
            v-if="currentIcon !== 'link'"
            icon="pi pi-times"
            text
            severity="secondary"
            rounded
            @click="currentIcon = 'link'"
            title="Reset icon"
        />
    </div>

    <Dialog
        v-model:visible="showIconPicker"
        header="Select Icon"
        :modal="true"
        :style="{ width: '520px' }"
        :draggable="false"
    >
        <div class="px-1 py-1">
            <div class="relative mb-3">
                <i
                    class="pi pi-search absolute left-3 top-1/2 -translate-y-1/2 text-gray-400"
                ></i>
                <InputText
                    v-model="iconSearchQuery"
                    placeholder="Search icons..."
                    class="w-full pl-10"
                    fluid
                />
            </div>
            <div class="max-h-[60vh] overflow-y-auto">
                <div
                    v-if="filteredIcons.length === 0"
                    class="text-center py-8 text-sm text-gray-400"
                >
                    No icons match "{{ iconSearchQuery }}"
                </div>
                <div v-else class="grid grid-cols-6 gap-2">
                    <Button
                        v-for="iconName in filteredIcons"
                        :key="iconName"
                        text
                        severity="secondary"
                        class="flex flex-col items-center gap-1 p-2"
                        :class="{
                            'bg-blue-50 border-blue-200':
                                currentIcon == iconName,
                        }"
                        :title="iconName"
                        @click="selectIcon(iconName)"
                    >
                        <span
                            class="material-symbols-outlined text-xl text-gray-600"
                            >{{ iconName }}</span
                        >
                        <span
                            class="text-[10px] text-gray-400 truncate w-full text-center leading-tight"
                            >{{ iconName }}</span
                        >
                    </Button>
                </div>
            </div>
        </div>
    </Dialog>
</template>
