<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
    usePoliciesStore,
    type PolicyWithPermissions,
    type PolicyPermission,
} from "@/stores/policies";
import { useCollectionsStore } from "@/stores/collections";
import { useToast } from "@/composables/useToast";
import type { FilterCondition } from "@/types/filters";
import Button from "primevue/button";
import Drawer from "primevue/drawer";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import InputText from "primevue/inputtext";
import Textarea from "primevue/textarea";
import DataTable from "primevue/datatable";
import Column from "primevue/column";
import Menu from "primevue/menu";
import Select from "primevue/select";
import MultiSelect from "primevue/multiselect";
import RadioButton from "primevue/radiobutton";
import FilterBuilder from "@/components/FilterBuilder.vue";
import { formatDate, actionSeverity } from "@/utils/formatters";
import Details from "@/components/policies/details.vue";
import PermissionRules from "@/components/policies/permissionRules.vue";

const route = useRoute(),
    store = usePoliciesStore(),
    collectionsStore = useCollectionsStore(),
    toast = useToast();

const policy = computed<PolicyWithPermissions | null>(
    () => store.currentPolicy,
);
const isNotFound = computed(
    () =>
        !store.detailLoading &&
        !policy.value &&
        (store.detailError?.toLowerCase().includes("not found") ||
            store.detailError?.toLowerCase().includes("http 4") ||
            store.detailError?.toLowerCase().includes("invalid")),
);

const assignedPlugins = ref<
    Array<{ plugin_slug: string; created_at?: string }>
>([]);

onMounted(async () => {
    const id = route.params.id as string;
    try {
        await store.getPolicy(id);
        const plugins = await store.fetchAssignedPlugins(id);
        assignedPlugins.value = plugins;
    } catch (e) {
        // error handled by store.detailError
    }
    if (collectionsStore.collections.length === 0) {
        await collectionsStore.fetchCollections();
    }
});
</script>

<template>
    <div class="p-6">
        <router-link
            to="/policies"
            class="inline-block mb-4 text-blue-500 text-sm hover:underline"
            >← Back to Policies</router-link
        >

        <div v-if="store.detailLoading" class="p-8 text-center text-gray-500">
            Loading policy...
        </div>
        <div v-else-if="isNotFound" class="text-center py-12">
            <div class="text-6xl mb-4">🔍</div>
            <h3 class="text-xl font-medium text-gray-900 mb-2">
                Policy not found
            </h3>
            <p class="text-gray-500 mb-4">
                The requested policy does not exist or has been removed.
            </p>
        </div>
        <div
            v-else-if="store.detailError"
            class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
        >
            {{ store.detailError }}
        </div>
        <template v-else-if="policy">
            <Details :policy="policy" />
            <PermissionRules :policy="policy" />

            <div class="bg-white p-6 rounded-lg shadow-sm">
                <h3 class="text-lg font-semibold text-gray-800 mb-4">
                    Assigned Plugins
                </h3>
                <div v-if="assignedPlugins.length > 0">
                    <DataTable
                        :value="assignedPlugins"
                        stripedRows
                        class="text-sm"
                    >
                        <Column header="Plugin Slug">
                            <template #body="{ data }">
                                <span class="font-medium text-gray-900">{{
                                    data.plugin_slug
                                }}</span>
                            </template>
                        </Column>
                        <Column header="Assigned Since">
                            <template #body="{ data }">
                                {{ formatDate(data.created_at) }}
                            </template>
                        </Column>
                    </DataTable>
                </div>
                <div v-else class="text-gray-400 italic py-4 text-center">
                    This policy is not assigned to any plugins
                </div>
            </div>
        </template>
    </div>
</template>
