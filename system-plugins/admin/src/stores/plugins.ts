import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type {
    Plugin,
    PluginSchemaResponse,
    MigrationStatus as SdkMigrationStatus,
    PluginPage as SdkPluginPage,
} from "alcedocore-sdk-node";
import { useAlcedoClient } from "../composables/useAlcedoClient";
import { withAsyncHandlingVoid } from "../utils/asyncUtils";

// Re-export types for external use
export type {
    Plugin,
    PluginSchemaResponse,
    MigrationStatus,
} from "alcedocore-sdk-node";

// Store-specific interface extending SDK types
export interface PluginStore extends Omit<Plugin, "type"> {
    plugin_type: "system" | "user";
    description?: string;
    displayName?: string;
    created_at?: string;
    updated_at?: string;
    endpoints?: Record<string, EndpointInfo>;
    settings?: {
        env_vars?: Record<string, string>;
        capabilities?: Record<string, unknown>;
        preferred_ram_mb?: number;
        preferred_cpu_ms?: number;
    };
    documentation?: string[];
}

// Endpoint info from the plugin record
export interface EndpointInfo {
    method: string;
    path?: string;
    protocol?: string;
}

export interface SettingsRequest {
    preferred_ram_mb?: number;
    preferred_cpu_ms?: number;
    env_vars?: Record<string, string>;
    capabilities?: Record<string, unknown>;
}

// Backend MigrationStatus response (wire format from /api/plugins/:slug/migrations)
//
// This type is intentionally NOT the SDK's `MigrationStatus`. The backend returns
// additional fields the SDK type does not model:
//   - `filename` (the .sql file basename)
//   - `status` ('applied' | 'pending' human-readable label)
//   - `applied` (boolean) and `applied_at` (RFC3339 timestamp)
//   - `has_down` (whether a corresponding down migration exists)
//   - `schema` (the Postgres schema name)
// The store exposes the SDK shape to views via `mapMigration()` (see below);
// richer fields are kept on the wire type for the few call-sites that need them.
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

// Backend page info (may include sidebar)
interface BackendPageInfo {
    path: string;
    label: string;
    icon: string;
    sidebar?: boolean;
}

// Backend schema response (different column format)
interface BackendTableInfo {
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
}

interface BackendSchemaResponse {
    plugin_name: string;
    schema_name: string;
    tables: BackendTableInfo[];
}

// Log entry from API
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

// Docker info response from API
export interface DockerInfoResponse {
    image: string;
    image_id: string;
    tags: string[];
    size: number;
    container_id: string | null;
    container_state: string | null;
    status: string;
}

// Version item from registry
export interface VersionItem {
    tag: string;
    size: number;
}

// List versions response
export interface ListVersionsResponse {
    versions: VersionItem[];
}

export interface InstanceInfo {
    task_id: string;
    slot: number;
    status: string;
    desired_state: string;
    container_id: string | null;
    node_id: string | null;
}

export interface InstanceDetail {
    task_id: string;
    slot: number;
    status: string;
    desired_state: string;
    container_id: string | null;
    container: {
        name: string;
        state: string;
        image: string;
        created: string;
        network_mode: string | null;
    } | null;
}

export interface ContainerStatsSnapshot {
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

// Map backend plugin to store format
function mapBackendPlugin(backendPlugin: {
    slug?: string;
    name?: string;
    version: string;
    enabled?: boolean;
    status?: string;
    type?: string;
    plugin_type?: string;
    description?: string;
    created_at?: string;
    updated_at?: string;
    endpoints?: Record<string, EndpointInfo>;
    documentation?: string[];
}): PluginStore {
    return {
        name: backendPlugin.slug || backendPlugin.name || "",
        version: backendPlugin.version,
        status:
            backendPlugin.status === "running"
                ? "enabled"
                : backendPlugin.status === "stopped"
                  ? "disabled"
                  : backendPlugin.enabled
                    ? "enabled"
                    : "disabled",
        plugin_type:
            backendPlugin.plugin_type === "static" ||
            backendPlugin.type === "static"
                ? "system"
                : "user",
        description: backendPlugin.description,
        created_at: backendPlugin.created_at,
        updated_at: backendPlugin.updated_at,
        endpoints: backendPlugin.endpoints,
        documentation: backendPlugin.documentation,
    };
}

// Map backend schema to SDK format
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

// Map backend migration to SDK format
function mapMigration(
    backendMigration: BackendMigrationStatus,
): SdkMigrationStatus {
    return {
        name: backendMigration.name,
        version: backendMigration.version,
        sql: backendMigration.sql,
        appliedAt:
            backendMigration.status === "applied"
                ? backendMigration.applied_at
                : null,
        pending: backendMigration.status === "pending",
    };
}

// Map backend page to SDK format
function mapPage(backendPage: BackendPageInfo): SdkPluginPage {
    return {
        path: backendPage.path,
        label: backendPage.label,
        icon: backendPage.icon,
        sidebar: backendPage.sidebar,
    };
}

export const usePluginsStore = defineStore("plugins", () => {
    const { client } = useAlcedoClient();

    const plugins = ref<PluginStore[]>([]);
    const currentPlugin = ref<PluginStore | null>(null);
    const loading = ref(false);
    const error = ref<string | null>(null);
    const pluginPagesMap = ref<Record<string, SdkPluginPage[]>>({});
    const pluginLoading = ref(false);
    const pluginError = ref<string | null>(null);
    const developerApiKeys = ref<any[]>([]);

    const totalPlugins = computed(() => plugins.value.length);
    const enabledPlugins = computed(() =>
        plugins.value.filter((p) => p.status === "enabled"),
    );
    const disabledPlugins = computed(() =>
        plugins.value.filter((p) => p.status === "disabled"),
    );
    const systemPlugins = computed(() =>
        plugins.value.filter((p) => p.plugin_type === "system"),
    );
    const userPlugins = computed(() =>
        plugins.value.filter((p) => p.plugin_type === "user"),
    );

    async function fetchPlugins() {
        await withAsyncHandlingVoid(loading, error, async () => {
            const response = await client.plugins.list();
            const pluginList =
                (response as any)?.data?.plugins ||
                (response as any)?.plugins ||
                [];
            plugins.value = pluginList.map(mapBackendPlugin);
        });
    }

    async function enablePlugin(name: string) {
        await client.plugins.enable(name);
        await fetchPlugins();
    }

    async function disablePlugin(name: string) {
        await client.plugins.disable(name);
        await fetchPlugins();
    }

    async function uninstallPlugin(name: string) {
        await client.plugins.uninstall(name);
        await fetchPlugins();
    }

    async function deletePlugin(name: string) {
        await client.plugins.delete(name);
        await fetchPlugins();
    }

    async function installPlugin(formData: FormData) {
        const zipFile = formData.get("zip") as File;
        await client.plugins.install(zipFile);
        await fetchPlugins();
    }

    async function createPlugin(formData: FormData): Promise<PluginStore> {
        const response = await client.plugins.create(formData);
        await fetchPlugins();
        return response;
    }

    async function addFromRegistry(
        slug: string,
        image: string,
    ): Promise<PluginStore> {
        const data = await client.plugins.createFromRegistry({
            slug,
            image,
            status: "disabled",
        });
        await fetchPlugins();
        return data.data;
    }

    async function fetchPluginDetail(name: string): Promise<PluginStore> {
        pluginLoading.value = true;
        pluginError.value = null;
        try {
            const response = await client.plugins.get(name);
            const detail = response?.data || response;
            currentPlugin.value = mapBackendPlugin(detail);
            return currentPlugin.value;
        } catch (e) {
            pluginError.value =
                e instanceof Error
                    ? e.message
                    : "Failed to fetch plugin detail";
            throw e;
        } finally {
            pluginLoading.value = false;
        }
    }

    async function fetchPluginSchema(
        name: string,
    ): Promise<PluginSchemaResponse> {
        const backendSchema = await client.plugins.schema(name);
        return mapSchema(backendSchema);
    }

    async function fetchPluginMigrations(
        name: string,
    ): Promise<{ migrations: SdkMigrationStatus[] }> {
        const response = await client.migrations.list(name);
        if (Array.isArray(response)) {
            return { migrations: response.map(mapMigration) };
        }
        const data =
            (response as { migrations?: BackendMigrationStatus[] })
                .migrations || [];
        return { migrations: data.map(mapMigration) };
    }

    async function runPendingMigrations(name: string): Promise<any> {
        return await client.migrations.run(name);
    }

    async function rollbackMigration(
        name: string,
        version: string,
    ): Promise<any> {
        return await client.migrations.rollback(name, version);
    }

    async function fetchPluginAssets(
        name: string,
    ): Promise<{ css: string; js: string }> {
        return await client.plugins.assets(name);
    }

    async function fetchPluginPages(name: string): Promise<SdkPluginPage[]> {
        const pages = await client.plugins.pages(name);
        const mapped = pages.map(mapPage);
        pluginPagesMap.value[name] = mapped;
        return mapped;
    }

    function getCachedPluginPages(name: string): SdkPluginPage[] {
        return pluginPagesMap.value[name] || [];
    }

    async function savePluginSettings(
        name: string,
        settings: Record<string, unknown>,
    ): Promise<PluginStore> {
        await client.settings.update(name, settings);
        if (!currentPlugin.value) {
            throw new Error(`Plugin "${name}" not found in store`);
        }
        return currentPlugin.value;
    }

    async function fetchPluginSettings(name: string): Promise<{
        settings: Record<string, unknown>;
        schema: Record<string, unknown> | null;
    }> {
        return (await client.settings.get(name)) as {
            settings: Record<string, unknown>;
            schema: Record<string, unknown> | null;
        };
    }

    // DOCS-03/API: list available documentation for a plugin
    async function fetchPluginDocs(name: string): Promise<{
        plugin: string;
        docs: Array<{ path: string; size: number }>;
    }> {
        const response = await client.plugins.docs(name);
        return response.data || { plugin: name, docs: [] };
    }

    // DOCS-04/API: fetch a specific doc's markdown content
    async function fetchDocContent(
        name: string,
        docPath: string,
    ): Promise<string> {
        const content = await client.plugins.docContent(name, docPath);
        return content;
    }

    // LOGS-01/API: fetch request logs for a plugin
    async function fetchPluginLogs(
        name: string,
        options?: {
            limit?: number;
            cursor?: string;
            status_code?: number;
            path?: string;
        },
    ): Promise<LogsResponse> {
        const params = new URLSearchParams();
        if (options?.limit) params.set("limit", String(options.limit));
        if (options?.cursor) params.set("cursor", options.cursor);
        if (options?.status_code)
            params.set("status_code", String(options.status_code));
        if (options?.path) params.set("path", options.path);

        const response = await client.plugins.requestLogs(name, params);
        return response.data || { logs: [], next_cursor: null };
    }

    // LOGS-02/API: fetch log detail with host calls
    async function fetchPluginLogDetail(
        name: string,
        requestId: string,
    ): Promise<LogDetailResponse> {
        const response = await client.plugins.requestLogDetail(name, requestId);
        return response.data || { request: null as any, host_calls: [] };
    }

    // DOCKER-01/API: fetch Docker image info for a plugin
    async function fetchPluginDockerInfo(
        name: string,
    ): Promise<{ data?: DockerInfoResponse }> {
        const response = await client.plugins.runtimeInfo(name);
        return response as { data?: DockerInfoResponse };
    }

    // VERSIONS-01/API: fetch available versions from registry for a plugin
    async function fetchPluginVersions(
        name: string,
    ): Promise<ListVersionsResponse> {
        const response = await client.plugins.versions(name);
        return response.data || { versions: [] };
    }

    // VERSIONS-02/API: deploy a specific version by tag
    async function deployPluginVersion(
        name: string,
        tag: string,
    ): Promise<{ data?: { status: string } }> {
        const response = await client.plugins.deploy(name, tag);
        return response as { data?: { status: string } };
    }

    // INSTANCES-01/API: list all instances for a plugin
    async function fetchPluginInstances(slug: string): Promise<InstanceInfo[]> {
        const json = await client.plugins.instances(slug);
        return json.data?.instances || [];
    }

    // INSTANCES-02/API: get detail for a specific instance
    async function fetchInstanceDetail(
        slug: string,
        taskId: string,
    ): Promise<InstanceDetail> {
        const json = await client.plugins.instance(slug, taskId);
        return json.data;
    }

    // INSTANCES-03/API: get CPU/memory stats snapshot for an instance
    async function fetchInstanceStats(
        slug: string,
        taskId: string,
    ): Promise<ContainerStatsSnapshot> {
        const json = await client.plugins.instanceStats(slug, taskId);
        return json.data;
    }

    // INSTANCES-04/API: get logs for a specific instance
    async function fetchInstanceLogs(
        slug: string,
        taskId: string,
    ): Promise<string[]> {
        return (await client.plugins.instanceLogs(slug, taskId)) as string[];
    }

    // INSTANCES-05/API: scale a plugin to a given number of replicas
    async function scalePlugin(
        slug: string,
        replicas: number,
        resourceLimits?: { cpu_limit: number; memory_limit: number },
    ): Promise<void> {
        await client.plugins.scale(slug, {
            replicas,
            resource_limits: resourceLimits,
        });
    }

    async function restartPlugin(
        name: string,
        containerId?: string,
    ): Promise<any> {
        return await client.plugins.restart(name, containerId);
    }

    // SCOPES-01: fetch scopes for a plugin
    async function fetchPluginScopes(slug: string): Promise<ScopesResponse> {
        const json = await client.plugins.scopes(slug);
        return json.data || { requested_scopes: [], granted_scopes: [] };
    }

    // SCOPES-02: update granted scopes
    async function updatePluginScopes(
        slug: string,
        scopes: string[],
    ): Promise<void> {
        await client.plugins.updateScopes(slug, scopes);
    }

    return {
        plugins,
        loading,
        error,
        currentPlugin,
        pluginLoading,
        pluginError,
        totalPlugins,
        enabledPlugins,
        disabledPlugins,
        systemPlugins,
        userPlugins,
        fetchPlugins,
        enablePlugin,
        disablePlugin,
        uninstallPlugin,
        deletePlugin,
        installPlugin,
        createPlugin,
        addFromRegistry,
        fetchPluginDetail,
        fetchPluginSchema,
        fetchPluginMigrations,
        fetchPluginAssets,
        fetchPluginPages,
        getCachedPluginPages,
        pluginPagesMap,
        savePluginSettings,
        fetchPluginSettings,
        fetchPluginDocs,
        fetchDocContent,
        fetchPluginLogs,
        fetchPluginLogDetail,
        fetchPluginDockerInfo,
        fetchPluginVersions,
        deployPluginVersion,
        rollbackMigration,
        runPendingMigrations,
        fetchPluginInstances,
        fetchInstanceDetail,
        fetchInstanceStats,
        fetchInstanceLogs,
        scalePlugin,
        fetchPluginScopes,
        updatePluginScopes,
        developerApiKeys,
        restartPlugin,
    };
});
