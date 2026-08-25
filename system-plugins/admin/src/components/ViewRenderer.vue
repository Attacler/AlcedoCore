<script setup lang="ts">
import { computed } from "vue";
import { useSavedViewsStore } from "@/stores/savedViews";
import { useSettingsStore } from "@/stores/settingsStore";
import type { FieldDefinition } from "@/stores/collections";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";

const savedViewsStore = useSavedViewsStore(),
    settingsStore = useSettingsStore(),
    extensionStore = useExtensionRegistryStore();

defineEmits<{
    "update:sort": [field: string, order: "asc" | "desc"];
    "update:page": [page: number];
    "update:per-page": [perPage: number];
    "delete-item": [item: any];
    retry: [];
    "add-item": [];
}>();

const props = defineProps<{
    items: any[];
    fields: FieldDefinition[];
    collectionName: string;
    loading: boolean;
    error: string | null;
    total: number;
    page: number;
    perPage: number;
    sortField: string;
    sortOrder: "asc" | "desc";
    filterCondition?: any;
    overrideRenderMode?: string | null;
    embedded?: boolean;
    enableExpand?: boolean;
    collectionFields?: FieldDefinition[];
    viewSpecific?: Record<string, any>;
    lazy?: boolean;
    rowLinkTo?: (item: any) => string;
    defaultViewConfig?: {
        render_mode?: string;
        view_specific?: Record<string, any>;
    };
}>();

const renderMode = computed(() => {
    if (props.overrideRenderMode) {
        return props.overrideRenderMode;
    }

    if (savedViewsStore.activeView?.config.render_mode) {
        return savedViewsStore.activeView.config.render_mode;
    }

    if (props.defaultViewConfig?.render_mode) {
        return props.defaultViewConfig.render_mode;
    }

    const defaultMode = settingsStore.getSettingValue("default_view_mode");
    if (defaultMode) {
        return defaultMode;
    }

    return "table";
});

const currentViewComponent = computed(() => {
    const mode = renderMode.value;
    if (!mode) return null;

    const viewEntry = extensionStore.getView(mode);

    if (viewEntry) return viewEntry.component;

    return null;
});
</script>

<template>
    <div v-if="!renderMode" class="text-center py-12">
        <p class="text-gray-500">Select a view type to display data.</p>
    </div>

    <div v-else-if="loading" class="flex items-center justify-center py-16">
        <svg
            class="animate-spin h-8 w-8 text-blue-500"
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 24 24"
        >
            <circle
                class="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                stroke-width="4"
            ></circle>
            <path
                class="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
            ></path>
        </svg>
    </div>

    <!-- Error State -->
    <div v-else-if="error" class="text-center py-8">
        <p class="text-red-500 mb-4">{{ error }}</p>
        <div v-if="error.includes('system collection')" class="mt-4">
            <p class="text-gray-500 text-sm mb-3">
                This collection has a dedicated management interface.
            </p>
            <router-link
                :to="'/' + collectionName"
                class="text-blue-500 hover:underline text-sm font-medium"
            >
                &#8592; Go to {{ collectionName }} management
            </router-link>
        </div>
        <Button
            v-else
            label="Retry"
            severity="primary"
            @click="$emit('retry')"
        />
    </div>

    <!-- Empty State -->
    <div v-else-if="items.length === 0" class="text-center py-12">
        <h3 class="text-lg font-medium text-gray-900 mb-4">No items yet</h3>
        <p class="text-gray-500 mb-4">
            Items will appear here once they are created.
        </p>
        <Button
            label="Add Item"
            icon="pi pi-plus"
            severity="primary"
            @click="$emit('add-item')"
        />
    </div>

    <!-- Active View Component -->
    <Transition v-else name="fade" mode="out-in">
        <component
            :is="currentViewComponent"
            :items="items"
            :fields="fields"
            :collection-name="collectionName"
            :loading="loading"
            :error="error"
            :total="total"
            :page="page"
            :per-page="perPage"
            :sort-field="sortField"
            :sort-order="sortOrder"
            :filter-condition="filterCondition"
            :embedded="embedded"
            :enable-expand="enableExpand"
            :collection-fields="collectionFields"
            :view-specific="viewSpecific"
            :lazy="lazy"
            :row-link-to="rowLinkTo"
            @update:sort="
                (field: string, order: 'asc' | 'desc') =>
                    $emit('update:sort', field, order)
            "
            @update:page="(p: number) => $emit('update:page', p)"
            @update:per-page="(n: number) => $emit('update:per-page', n)"
            @delete-item="(i: any) => $emit('delete-item', i)"
        >
            <template
                v-for="(_, name) in $slots"
                :key="name"
                #[name]="slotProps"
            >
                <slot :name="name" v-bind="slotProps" />
            </template>
        </component>
    </Transition>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
    transition: opacity 0.15s ease;
}
.fade-enter-from,
.fade-leave-to {
    opacity: 0;
}
</style>
