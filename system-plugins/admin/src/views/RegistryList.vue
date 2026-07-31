<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRegistriesStore } from "@/stores/registries";
import Button from "primevue/button";
import { useRouter } from "vue-router";
import { computed } from "vue";

const store = useRegistriesStore(),
    router = useRouter();

const searchQuery = ref("");

onMounted(() => {
    store.fetchRegistries();
});

const filteredRegistries = computed(() => {
    return store.registries.filter(
        (registery) =>
            registery.name
                .toLowerCase()
                .includes(searchQuery.value.toLowerCase()) ||
            registery.url
                .toLowerCase()
                .includes(searchQuery.value.toLowerCase()) ||
            (registery.pull_url || "")
                .toLowerCase()
                .includes(searchQuery.value.toLowerCase()),
    );
});
</script>

<template>
    <div class="p-6">
        <div class="flex gap-4 mb-4 items-center">
            <IconField class="grow">
                <InputIcon class="pi pi-search" />
                <InputText
                    v-model="searchQuery"
                    placeholder="Search registries..."
                    class="flex-1"
                    fluid
                    autofocus
                />
            </IconField>
            <Button
                label="Add Registry"
                severity="primary"
                @click="router.push('/registries/new')"
                size="small"
            />
        </div>

        <!-- Loading State -->
        <div v-if="store.loading" class="text-center text-gray-500 py-8">
            Loading...
        </div>

        <!-- Error State -->
        <div v-else-if="store.error" class="text-center text-red-500 py-8">
            Failed to load: {{ store.error }}
        </div>

        <!-- Registry List -->
        <div
            v-else-if="store.registries.length > 0"
            class="bg-white rounded-lg shadow-sm overflow-hidden"
        >
            <router-link
                v-for="registry in filteredRegistries"
                :key="registry.id"
                class="flex items-center p-4 gap-4 border-b border-gray-200 hover:bg-gray-50 transition-colors"
                :to="`/registries/${registry.id}`"
            >
                <span class="grow font-medium text-gray-900">{{
                    registry.name
                }}</span>
                <div class="text-sm text-gray-500 mt-1">
                    {{ registry.url }}
                </div>
            </router-link>
        </div>

        <!-- Empty State -->
        <div v-else class="text-center py-12">
            <h3 class="text-lg font-medium text-gray-900 mb-2">
                No registries found
            </h3>
            <p class="text-gray-500 mb-6">Add a registry to get started</p>
            <Button
                label="Add Registry"
                severity="primary"
                as="router-link"
                to="/registries/new"
                icon="pi pi-plus"
            />
        </div>
    </div>
</template>
