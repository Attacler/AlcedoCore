import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { useAlcedoClient } from "../composables/useAlcedoClient";
import type {
    RouteMapping,
    CreateRouteMappingRequest,
    UpdateRouteMappingRequest,
} from "@alcedocore/sdk";

export const useRouteMappingsStore = defineStore("routeMappings", () => {
    const { client } = useAlcedoClient();

    const mappings = ref<RouteMapping[]>([]);
    const loading = ref(false);
    const error = ref<string | null>(null);

    const totalMappings = computed(() => mappings.value.length);
    const activeMappings = computed(
        () => mappings.value.filter((m) => m.is_active).length,
    );

    async function fetchMappings() {
        loading.value = true;
        error.value = null;
        try {
            const response = (await client.routeMappings.list()) as {
                mappings?: RouteMapping[];
                data?: { mappings: RouteMapping[] };
            };
            mappings.value =
                response.mappings || response.data?.mappings || [];
        } catch (e) {
            error.value =
                e instanceof Error ? e.message : "Failed to fetch route mappings";
        } finally {
            loading.value = false;
        }
    }

    async function createMapping(data: CreateRouteMappingRequest) {
        const response = (await client.routeMappings.create(data)) as
            | { data?: RouteMapping }
            | RouteMapping;
        await fetchMappings();
        return "data" in response && response.data
            ? response.data
            : (response as RouteMapping);
    }

    async function updateMapping(id: number, data: UpdateRouteMappingRequest) {
        const response = (await client.routeMappings.update(id, data)) as
            | { data?: RouteMapping }
            | RouteMapping;
        await fetchMappings();
        return "data" in response && response.data
            ? response.data
            : (response as RouteMapping);
    }

    async function deleteMapping(id: number) {
        await client.routeMappings.delete(id);
        await fetchMappings();
    }

    async function toggleActive(mapping: RouteMapping) {
        await updateMapping(mapping.id, { is_active: !mapping.is_active });
    }

    return {
        mappings,
        loading,
        error,
        totalMappings,
        activeMappings,
        fetchMappings,
        createMapping,
        updateMapping,
        deleteMapping,
        toggleActive,
    };
});
