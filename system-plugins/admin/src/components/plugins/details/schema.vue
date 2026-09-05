<script lang="ts" setup>
import SchemaErDiagram from "./SchemaErDiagram.vue";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import { formatDate } from "@/utils/formatters";
import type { PluginSchemaResponse, MigrationStatus } from "@alcedocore/sdk";
import { onMounted } from "vue";
import { ref } from "vue";
import { useRoute } from "vue-router";
import { useToast } from "@/composables/useToast";
import { usePluginsStore, type PluginStore } from "@/stores/plugins";

const props = defineProps<{ plugin: PluginStore }>();

const route = useRoute(),
    store = usePluginsStore(),
    toast = useToast();

const expandedMigration = ref<number | null>(null),
    rollbackLoading = ref(false),
    rollbackVersion = ref<string | null>(null),
    showRollbackDialog = ref(false),
    rollbackMigrationData = ref<any>(null);

const schema = ref<PluginSchemaResponse | null>(null),
    migrations = ref<MigrationStatus[] | null>(null),
    schemaLoading = ref(false),
    migrationsLoading = ref(false),
    schemaError = ref<string | null>(null),
    migrationsError = ref<string | null>(null);

function toggleMigration(idx: number) {
    expandedMigration.value = expandedMigration.value === idx ? null : idx;
}

async function loadSchema() {
    await withAsyncHandlingVoid(
        schemaLoading,
        schemaError,
        async () => {
            schema.value = await store.fetchPluginSchema(
                route.params.name as string,
            );
        },
        "Failed to load schema",
    );
}

async function loadMigrations() {
    await withAsyncHandlingVoid(
        migrationsLoading,
        migrationsError,
        async () => {
            const res = await store.fetchPluginMigrations(
                route.params.name as string,
            );
            migrations.value = res.migrations;
        },
        "Failed to load migrations",
    );
}

function openRollbackDialog(migration: any) {
    rollbackMigrationData.value = migration;
    showRollbackDialog.value = true;
}

async function runPendingMigrations() {
    await store.runPendingMigrations(route.params.name as string);
    await loadMigrations();
    await loadSchema();
}

async function doRollback() {
    const migration = rollbackMigrationData.value;
    if (!migration) return;
    showRollbackDialog.value = false;
    rollbackLoading.value = true;
    rollbackVersion.value = migration.version;
    try {
        await store.rollbackMigration(
            route.params.name as string,
            migration.version,
        );
        toast.show(
            `Rollback completed for version ${migration.version}`,
            "success",
        );
        await loadMigrations();
        await loadSchema();
    } catch (e) {
        toast.show(
            `Rollback failed: ${e instanceof Error ? e.message : e}`,
            "error",
        );
    } finally {
        rollbackLoading.value = false;
        rollbackVersion.value = null;
        rollbackMigrationData.value = null;
    }
}

onMounted(() => {
    loadSchema();
    loadMigrations();
});
</script>

<template>
    <div>
        <div v-if="migrationsLoading" class="text-gray-500">
            Loading migrations...
        </div>
        <div
            v-else-if="migrationsError"
            class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
        >
            {{ migrationsError }}
        </div>
        <div v-else-if="migrations && migrations.length > 0">
            <div
                v-for="(m, idx) in migrations"
                :key="m.version"
                class="border-b border-gray-200 last:border-b-0"
            >
                <div class="flex items-center gap-2 p-3">
                    <button
                        class="flex-1 flex gap-4 items-center hover:bg-gray-50 transition-colors text-left rounded flex-wrap"
                        @click="toggleMigration(idx)"
                    >
                        <span class="font-mono text-sm text-gray-500">{{
                            m.version
                        }}</span>
                        <span class="flex-1">{{ m.name }}</span>
                        <span
                            class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
                            :class="{
                                'bg-green-100 text-green-800': !m.pending,
                                'bg-yellow-100 text-yellow-800': m.pending,
                            }"
                            >{{ m.pending ? "pending" : "applied" }}</span
                        >
                        <span
                            v-if="m.appliedAt"
                            class="text-xs text-gray-400"
                            >{{ formatDate(m.appliedAt) }}</span
                        >
                        <svg
                            class="w-4 h-4 text-gray-400 transition-transform"
                            :class="{
                                'rotate-180': expandedMigration === idx,
                            }"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                        >
                            <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                stroke-width="2"
                                d="M19 9l-7 7-7-7"
                            />
                        </svg>
                    </button>
                    <Button
                        v-if="!m.pending"
                        :label="
                            rollbackLoading && rollbackVersion === m.version
                                ? 'Rolling back...'
                                : 'Rollback'
                        "
                        severity="danger"
                        size="small"
                        :disabled="rollbackLoading"
                        @click="openRollbackDialog(m)"
                    />
                </div>
                <div v-if="expandedMigration === idx" class="px-4 pb-4">
                    <div class="flex gap-2 text-xs text-gray-500 mb-2">
                        <span>Version: {{ m.version }}</span>
                        <span v-if="m.appliedAt"
                            >| Applied: {{ formatDate(m.appliedAt) }}</span
                        >
                    </div>
                    <div
                        class="p-3 bg-gray-50 rounded-lg border border-gray-200"
                    >
                        <pre
                            class="text-xs text-gray-600 whitespace-pre-wrap font-mono"
                            >{{ m.sql }}</pre
                        >
                    </div>
                </div>
            </div>
            <Button
                v-if="migrations.find((e: any) => e.pending)"
                label="Run pending migrations"
                @click="runPendingMigrations"
                class="float-right mt-2"
                size="small"
            />
        </div>
        <div v-else class="text-gray-400 italic">
            No migrations available for this plugin
        </div>

        <div v-if="schemaLoading" class="text-gray-500">Loading schema...</div>
        <div
            v-else-if="schemaError"
            class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
        >
            {{ schemaError }}
        </div>
        <div
            v-else-if="schema && schema.tables && schema.tables.length > 0"
            ref="schemaTabContent"
        >
            <SchemaErDiagram :schema="schema" />
        </div>
        <div v-else class="text-gray-400 italic">
            No schema available for this plugin
        </div>

        <Dialog
            v-model:visible="showRollbackDialog"
            header="Confirm Rollback"
            :modal="true"
            :style="{ width: '450px' }"
            :draggable="false"
        >
            <p class="text-gray-600">
                Rollback migration
                <strong>{{ rollbackMigrationData?.version }}</strong> ({{
                    rollbackMigrationData?.name
                }})? This will execute the down migration.
            </p>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="showRollbackDialog = false"
                />
                <Button
                    label="Rollback"
                    severity="danger"
                    :loading="rollbackLoading"
                    @click="doRollback"
                />
            </template>
        </Dialog>
    </div>
</template>
