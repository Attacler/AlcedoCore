import { useExtensionRegistryStore } from "./extensionRegistry";
import { defineStore } from "pinia";
import { ref, computed, nextTick } from "vue";
import type {
    Plugin,
    PluginSchemaResponse,
    MigrationStatus as SdkMigrationStatus,
    PluginPage as SdkPluginPage,
} from "@alcedocore/sdk";
import { useAlcedoClient } from "../composables/useAlcedoClient";
import { withAsyncHandlingVoid } from "../utils/asyncUtils";
import TableViewSettings from "@/views/TableViewSettings.vue";
import TableView from "@/views/TableView.vue";
import CardsView from "@/views/CardsView.vue";
import KanbanView from "@/views/KanbanView.vue";
import KanbanViewSettings from "@/views/KanbanViewSettings.vue";
import CardsViewSettings from "@/views/CardsViewSettings.vue";

export type {
    Plugin,
    PluginSchemaResponse,
    MigrationStatus,
} from "@alcedocore/sdk";

export interface EndpointInfo {
    method: string;
    path?: string;
    protocol?: string;
}

/** One plugin deployment (plugin × app version). */
export interface InstallStore {
    id: number;
    app_version_id: number;
    app_id?: number | null;
    app_name?: string | null;
    api_name?: string | null;
    version_id?: number | null;
    version_name?: string | null;
    plugin_version: string;
    enabled: boolean;
    settings: Record<string, unknown>;
    granted_scopes: string[];
    created_at?: string;
    updated_at?: string;
}

export interface PluginStore {
    id: number;
    /** Plugin slug — components read `name`. */
    name: string;
    slug: string;
    plugin_type: "system" | "user";
    registry_id: number;
    registry?: string | null;
    image?: string | null;
    description?: string;
    endpoints?: Record<string, EndpointInfo>;
    documentation?: string[];
    requested_scopes?: Array<{ name: string; description?: string }>;
    installations: InstallStore[];
    /** Derived: any install enabled. */
    status: "enabled" | "disabled";
    /** Derived: newest deployed plugin tag (list column). */
    version: string;
    created_at?: string;
    updated_at?: string;
}

interface BackendMigrationStatus {
    version: string;
    name: string;
    filename: string;
    status: "applied" | "pending";
    applied: boolean;
    applied_at: string | null;
    has_down: boolean;
    sql: string;
    schema: string;
}

interface BackendSchemaResponse {
    plugin_name: string;
    schema_name: string;
    tables: {
        table_name: string;
        columns: {
            column_name: string;
            data_type: string;
            is_nullable: string;
            column_default: string | null;
        }[];
        primary_key: {
            constraint_name: string;
            columns: string[];
        } | null;
        foreign_keys: {
            constraint_name: string;
            column_name: string;
            foreign_table_schema: string;
            foreign_table_name: string;
            foreign_column_name: string;
        }[];
    }[];
}

export interface RequestLogEntry {
    request_uuid: string;
    plugin_name: string;
    method: string;
    path: string;
    status_code: number;
    duration_ms: number;
    created_at: string;
}

export interface LogsResponse {
    logs: RequestLogEntry[];
    next_cursor: string | null;
}

export interface HostCallEntry {
    id: number;
    parent_request_id: string;
    action_type: string;
    args_summary: string;
    result_summary: string;
    duration_ms: number;
    created_at: string;
}

export interface LogDetailResponse {
    request: RequestLogEntry;
    host_calls: HostCallEntry[];
}

export interface DockerInfoResponse {
    image: string;
    image_id: string;
    tags: string[];
    size: number;
    deployment_id: string | null;
    deployment_state: string | null;
    status: string;
}

export interface VersionItem {
    tag: string;
    size: number;
}

export interface ListVersionsResponse {
    versions: VersionItem[];
}

export interface InstanceInfo {
    task_id: string;
    slot: number;
    status: string;
    desired_state: string;
    deployment_id: string | null;
    node_id: string | null;
}

export interface InstanceDetail {
    task_id: string;
    slot: number;
    status: string;
    desired_state: string;
    deployment_id: string | null;
    deployment: {
        name: string;
        state: string;
        image: string;
        created: string;
        network_mode: string | null;
    } | null;
}

export interface DeploymentStatsSnapshot {
    timestamp: string;
    cpu_percent: number;
    memory_usage_bytes: number;
}

export interface ScopeDef {
    name: string;
    description: string;
}

export interface ScopesResponse {
    requested_scopes: ScopeDef[];
    granted_scopes: string[];
}

export interface DeployPayload {
    slug: string;
    plugin_version: string;
    registry_id?: number;
    /** Image repository name, for wizard prefill. */
    image?: string;
    /** Target app × version pair (alcedocore_apps_versions.id). */
    app_version_id: number;
    plugin_type?: string;
    description?: string;
    endpoints?: Record<string, EndpointInfo>;
    requested_scopes?: Array<{ name: string; description?: string }>;
    granted_scopes?: string[];
    settings?: Record<string, unknown>;
    enabled?: boolean;
}

function installFromBackend(i: any): InstallStore {
    return {
        id: i.id,
        app_version_id: i.app_version_id,
        app_id: i.app_id ?? null,
        app_name: i.app_name ?? null,
        api_name: i.api_name ?? null,
        version_id: i.version_id ?? null,
        version_name: i.version_name ?? null,
        plugin_version: i.plugin_version,
        enabled: !!i.enabled,
        settings: i.settings || {},
        granted_scopes: i.granted_scopes || [],
        created_at: i.created_at,
        updated_at: i.updated_at,
    };
}

function mapBackendPlugin(p: any): PluginStore {
    const installations: InstallStore[] = (p.installations || []).map(
        installFromBackend,
    );
    const enabled = installations.some((i) => i.enabled);
    return {
        id: p.id,
        slug: p.slug,
        name: p.slug,
        plugin_type: p.plugin_type === "static" ? "system" : "user",
        registry_id: p.registry_id ?? 0,
        registry: p.registry ?? null,
        image: p.image ?? null,
        description: p.description,
        endpoints: p.endpoints,
        documentation: p.documentation,
        requested_scopes: p.requested_scopes,
        installations,
        status: enabled ? "enabled" : "disabled",
        version: installations.length
            ? installations[installations.length - 1].plugin_version
            : "",
        created_at: p.created_at,
        updated_at: p.updated_at,
    };
}

function mapSchema(backendSchema: BackendSchemaResponse): PluginSchemaResponse {
    return {
        tables: backendSchema.tables.map((t) => ({
            name: t.table_name,
            columns: t.columns.map((c) => ({
                name: c.column_name,
                type: c.data_type,
                nullable: c.is_nullable === "YES",
                default: c.column_default ?? null,
            })),
            primaryKey: t.primary_key
                ? {
                      constraintName: t.primary_key.constraint_name,
                      columns: t.primary_key.columns,
                  }
                : undefined,
            foreignKeys: t.foreign_keys.map((fk) => ({
                constraintName: fk.constraint_name,
                columnName: fk.column_name,
                foreignTableName: fk.foreign_table_name,
                foreignColumnName: fk.foreign_column_name,
            })),
        })),
    };
}

function mapMigration(m: BackendMigrationStatus): SdkMigrationStatus {
    return {
        name: m.name,
        version: m.version,
        sql: m.sql,
        appliedAt: m.status === "applied" ? m.applied_at : null,
        pending: m.status === "pending",
    };
}

export const usePluginsStore = defineStore("plugins", () => {
    const { client } = useAlcedoClient();
    const extensionRegistry = useExtensionRegistryStore();

    const plugins = ref<PluginStore[]>([]),
        currentPlugin = ref<PluginStore | null>(null),
        loading = ref(false),
        error = ref<string | null>(null),
        pluginPagesMap = ref<Record<string, SdkPluginPage[]>>({}),
        pluginLoading = ref(false);

    const pluginsReady = ref(false);
    const registeredPlugins = new Set<string>();

    const totalPlugins = computed(() => plugins.value.length),
        enabledPlugins = computed(() =>
            plugins.value.filter((p) => p.status === "enabled"),
        ),
        disabledPlugins = computed(() =>
            plugins.value.filter((p) => p.status === "disabled"),
        ),
        systemPlugins = computed(() =>
            plugins.value.filter((p) => p.plugin_type === "system"),
        ),
        userPlugins = computed(() =>
            plugins.value.filter((p) => p.plugin_type === "user"),
        );

    /** Remove every plugin registration and injected stylesheet from a previous context. */
    function resetRegistrations() {
        for (const slug of registeredPlugins) {
            extensionRegistry.unregisterPlugin(slug);
            document.querySelector(`style[cid='${slug}']`)?.remove();
        }
        registeredPlugins.clear();
        pluginPagesMap.value = {};
    }

    async function fetchPlugins(params?: { app?: string; version?: string }) {
        resetRegistrations();
        pluginsReady.value = false;
        await withAsyncHandlingVoid(loading, error, async () => {
            const searchParams: Record<string, string> = {};
            if (params?.app) searchParams.app = params.app;
            if (params?.version) searchParams.version = params.version;

            const response = await client.plugins.list({ searchParams });
            const pluginList =
                (response as any)?.data?.plugins ||
                (response as any)?.plugins ||
                [];
            plugins.value = pluginList.map(mapBackendPlugin);
            await nextTick();
            // ponytail: plugin asset loading (pages/inputs/displays) is deferred.
        });
        pluginsReady.value = true;
    }

    async function fetchPluginDetail(slug: string): Promise<PluginStore> {
        pluginLoading.value = true;
        try {
            const response = await client.plugins.get(slug);
            const detail = (response as any)?.data || response;
            currentPlugin.value = mapBackendPlugin(detail);
            return currentPlugin.value;
        } finally {
            pluginLoading.value = false;
        }
    }

    async function deployPlugin(payload: DeployPayload): Promise<PluginStore> {
        const response: any = await client.plugins.deploy(payload);
        const detail = response?.data || response;
        await fetchPlugins();
        return mapBackendPlugin(detail);
    }

    async function uninstallPlugin(slug: string, appVersionId: number) {
        await client.plugins.uninstall(slug, appVersionId);
        await fetchPlugins();
    }

    async function setInstallEnabled(
        slug: string,
        appVersionId: number,
        enabled: boolean,
    ) {
        if (enabled) await client.plugins.enable(slug, appVersionId);
        else await client.plugins.disable(slug, appVersionId);
    }

    /** Toggle the newest install (convenience for the list/detail actions). */
    async function enablePlugin(slug: string) {
        const install = currentPlugin.value?.installations[0];
        if (install) await setInstallEnabled(slug, install.app_version_id, true);
    }

    async function disablePlugin(slug: string) {
        const install = currentPlugin.value?.installations[0];
        if (install) await setInstallEnabled(slug, install.app_version_id, false);
    }

    async function deletePlugin(slug: string) {
        await client.plugins.delete(slug);
        await fetchPlugins();
    }

    async function fetchPluginSchema(slug: string): Promise<PluginSchemaResponse> {
        const backend = await client.plugins.schema(slug);
        return mapSchema(backend as unknown as BackendSchemaResponse);
    }

    async function fetchPluginMigrations(slug: string) {
        const response = await client.migrations.list(slug);
        if (Array.isArray(response)) {
            return { migrations: response.map(mapMigration) };
        }
        const data = (response as { migrations?: BackendMigrationStatus[] })
            .migrations || [];
        return { migrations: data.map(mapMigration) };
    }

    async function runPendingMigrations(slug: string): Promise<any> {
        return await client.migrations.run(slug);
    }

    async function rollbackMigration(slug: string, version: string) {
        return await client.migrations.rollback(slug, version);
    }

    async function fetchPluginAssets(slug: string): Promise<null | any> {
        const assets = await client.plugins.assets(slug);
        if (!assets?.js) return null;
        const blob = new Blob([assets.js], { type: "application/javascript" });
        const url = URL.createObjectURL(blob);
        const module = await import(/* @vite-ignore */ url);
        URL.revokeObjectURL(url);

        const existingCSS = document.querySelector("style[cid='" + slug + "']");
        if (existingCSS) existingCSS.remove();
        if (assets.css) {
            const style = document.createElement("style");
            style.innerHTML = assets.css;
            style.setAttribute("cid", slug);
            document.head.appendChild(style);
        }
        return module;
    }

    async function fetchPluginPages(slug: string): Promise<SdkPluginPage[]> {
        const pages = await client.plugins.pages(slug);
        pluginPagesMap.value[slug] = pages as SdkPluginPage[];
        return pages as SdkPluginPage[];
    }

    function getCachedPluginPages(slug: string): SdkPluginPage[] {
        return pluginPagesMap.value[slug] || [];
    }

    // --- install settings / scopes (version-scoped) ---

    async function fetchPluginSettings(slug: string, appVersionId: number) {
        return (await client.plugins.getSettings(slug, appVersionId)) as {
            settings: Record<string, unknown>;
            schema: Record<string, unknown> | null;
        };
    }

    async function savePluginSettings(
        slug: string,
        appVersionId: number,
        settings: Record<string, unknown>,
    ): Promise<void> {
        await client.plugins.updateSettings(slug, appVersionId, settings);
    }

    async function fetchPluginScopes(
        slug: string,
        appVersionId: number,
    ): Promise<ScopesResponse> {
        const json: any = await client.plugins.getScopes(slug, appVersionId);
        return json.data || json;
    }

    async function updatePluginScopes(
        slug: string,
        appVersionId: number,
        scopes: string[],
    ): Promise<void> {
        await client.plugins.updateScopes(slug, appVersionId, scopes);
    }

    // --- docs / runtime (mocked empties, keyed by slug) ---

    async function fetchPluginDocs(slug: string) {
        const response: any = await client.plugins.docs(slug);
        return response.data || { plugin: slug, docs: [] };
    }

    async function fetchDocContent(slug: string, docPath: string) {
        return await client.plugins.docContent(slug, docPath);
    }

    async function fetchPluginLogs(slug: string, options?: { limit?: number; cursor?: string; status_code?: number; path?: string }) {
        const params = new URLSearchParams();
        if (options?.limit) params.set("limit", String(options.limit));
        if (options?.cursor) params.set("cursor", options.cursor);
        if (options?.status_code) params.set("status_code", String(options.status_code));
        if (options?.path) params.set("path", options.path);
        const response: any = await client.plugins.requestLogs(slug, params);
        return response.data || { logs: [], next_cursor: null };
    }

    async function fetchPluginDockerInfo(slug: string) {
        return (await client.plugins.runtimeInfo(slug)) as { data?: DockerInfoResponse };
    }

    async function fetchPluginVersions(slug: string) {
        const response: any = await client.plugins.versions(slug);
        return response.data || { versions: [] };
    }

    async function fetchPluginInstances(slug: string): Promise<InstanceInfo[]> {
        const json: any = await client.plugins.instances(slug);
        return json.data?.instances || [];
    }

    // Runtime detail draws — no runtime yet, return empties.
    async function fetchInstanceDetail(_slug: string, taskId: string): Promise<InstanceDetail> {
        return {
            task_id: taskId,
            slot: 0,
            status: "",
            desired_state: "",
            deployment_id: null,
            deployment: null,
        };
    }

    async function fetchInstanceStats(_slug: string, _taskId: string): Promise<DeploymentStatsSnapshot> {
        return {
            timestamp: new Date().toISOString(),
            cpu_percent: 0,
            memory_usage_bytes: 0,
        };
    }

    async function fetchInstanceLogs(_slug: string, _taskId: string): Promise<string[]> {
        return [];
    }

    async function fetchPluginLogDetail(_slug: string, _requestId: string): Promise<LogDetailResponse> {
        return { request: null as unknown as RequestLogEntry, host_calls: [] };
    }

    async function scalePlugin(slug: string, replicas: number, resourceLimits?: { cpu_limit: number; memory_limit: number }) {
        await client.plugins.scale(slug, { replicas, resource_limits: resourceLimits });
    }

    async function restartPlugin(slug: string, containerId?: string) {
        return await client.plugins.restart(slug, containerId);
    }

    async function deployPluginVersion(slug: string, tag: string) {
        // Re-deploy to the newest install's version.
        const install = currentPlugin.value?.installations[0];
        if (!install) throw new Error("Plugin has no install to redeploy");
        return await deployPlugin({
            slug,
            plugin_version: tag,
            registry_id: currentPlugin.value?.registry_id,
            app_version_id: install.app_version_id,
        });
    }

    return {
        plugins,
        loading,
        error,
        pluginsReady,
        currentPlugin,
        totalPlugins,
        enabledPlugins,
        disabledPlugins,
        systemPlugins,
        userPlugins,
        fetchPlugins,
        resetRegistrations,
        fetchPluginDetail,
        deployPlugin,
        uninstallPlugin,
        setInstallEnabled,
        enablePlugin,
        disablePlugin,
        deletePlugin,
        fetchPluginSchema,
        fetchPluginMigrations,
        fetchPluginAssets,
        fetchPluginPages,
        getCachedPluginPages,
        pluginPagesMap,
        fetchPluginSettings,
        savePluginSettings,
        fetchPluginScopes,
        updatePluginScopes,
        fetchPluginDocs,
        fetchDocContent,
        fetchPluginLogs,
        fetchPluginDockerInfo,
        fetchPluginVersions,
        fetchPluginInstances,
        fetchInstanceDetail,
        fetchInstanceStats,
        fetchInstanceLogs,
        fetchPluginLogDetail,
        deployPluginVersion,
        rollbackMigration,
        runPendingMigrations,
        scalePlugin,
        restartPlugin,
    };
});

/**
 * Register the built-in system view types (Table, Cards, Kanban) in the
 * extension registry.
 */
export function registerSystemViewTypes() {
    const registry = useExtensionRegistryStore();
    registry.registerViewType({
        component: TableView,
        label: "Table",
        pluginSlug: "system",
        type: "table",
        settingsComponent: TableViewSettings,
    });
    registry.registerViewType({
        component: CardsView,
        label: "Cards",
        pluginSlug: "system",
        type: "cards",
        settingsComponent: CardsViewSettings,
    });
    registry.registerViewType({
        component: KanbanView,
        label: "Kanban",
        pluginSlug: "system",
        type: "kanban",
        settingsComponent: KanbanViewSettings,
    });
}
