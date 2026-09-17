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

export interface PluginStore extends Omit<Plugin, "type"> {
    plugin_type: "system" | "user";
    description?: string;
    displayName?: string;
    name: string;
    created_at?: string;
    updated_at?: string;
    registry?: string | null;
    endpoints?: Record<string, EndpointInfo>;
    settings?: {
        env_vars?: Record<string, string>;
        capabilities?: Record<string, unknown>;
        preferred_ram_mb?: number;
        preferred_cpu_ms?: number;
    };
    documentation?: string[];
    scope?: "global" | "version" | "app";
    appVersion?: number | null;
    versionId?: number | null;
    installId?: number | null;
}

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

interface BackendPageInfo {
    path: string;
    label: string;
    icon: string;
    sidebar?: boolean;
}

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
    registry?: string | null;
    scope?: "global" | "version" | "app";
    app_version_id?: number | null;
    version_id?: number | null;
    id?: number;
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
        registry: backendPlugin.registry ?? null,
        endpoints: backendPlugin.endpoints,
        documentation: backendPlugin.documentation,
        scope:
            backendPlugin.scope ??
            (backendPlugin.app_version_id != null
                ? "app"
                : backendPlugin.version_id != null
                  ? "version"
                  : "global"),
        appVersion: backendPlugin.app_version_id ?? null,
        versionId: backendPlugin.version_id ?? null,
        installId: backendPlugin.id ?? null,
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
    const extensionRegistry = useExtensionRegistryStore();

    const plugins = ref<PluginStore[]>([]),
        currentPlugin = ref<PluginStore | null>(null),
        loading = ref(false),
        error = ref<string | null>(null),
        pluginPagesMap = ref<Record<string, SdkPluginPage[]>>({}),
        pluginLoading = ref(false),
        activeInstallId = ref<number | null>(null);

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
        ),
        globalPlugins = computed(() =>
            plugins.value.filter((p) => p.scope === "global"),
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

    /**
     * Merge the currently active install id into an SDK request's options.
     * Returns `extra` untouched when no specific install is active (e.g. app
     * zone, or before a plugin detail is loaded), so context resolution is
     * unaffected.
     */
    function installParam(extra?: any) {
        if (activeInstallId.value == null) return extra;
        return {
            ...(extra || {}),
            searchParams: {
                ...((extra && extra.searchParams) || {}),
                install_id: String(activeInstallId.value),
            },
        };
    }

    async function fetchPlugins(params?: {
        effective?: boolean;
        scope?: "global" | "version" | "app";
        version?: string;
        app?: string;
        version_id?: number;
    }) {
        resetRegistrations();
        activeInstallId.value = null;
        pluginsReady.value = false;
        await withAsyncHandlingVoid(loading, error, async () => {
            const searchParams: Record<string, string> = {};
            // Backend defaults to 20; request the max (capped at 100) so the
            // list is not silently truncated.
            searchParams.limit = "100";
            if (params?.effective) searchParams.effective = "true";
            if (params?.scope) searchParams.scope = params.scope;
            if (params?.version) searchParams.version = params.version;
            if (params?.app) searchParams.app = params.app;
            if (params?.version_id != null)
                searchParams.version_id = String(params.version_id);

            const response = await client.plugins.list({ searchParams });
            const pluginList =
                (response as any)?.data?.plugins ||
                (response as any)?.plugins ||
                [];
            const pluginArray = pluginList.map(mapBackendPlugin);
            plugins.value = pluginArray;
            await nextTick();

            for (const enabledPlugin of pluginArray.filter(
                (e: any) => e.status == "enabled",
            )) {
                console.log({ enabledPlugin });
                if (!enabledPlugin.name) continue;

                try {
                    const load = await fetchPluginAssets(enabledPlugin.name);

                    if (!load) continue;
                    registeredPlugins.add(enabledPlugin.name);
                    if (!load.default) continue;
                    console.log(enabledPlugin.name, { load });

                    for (const input of load.default.inputs || []) {
                        extensionRegistry.registerInputWidget({
                            component: input.component,
                            label: input.label,
                            supportedFieldTypes:
                                input.supportedFieldTypes || [],
                            pluginSlug: enabledPlugin.name,
                            type: input.name,
                            settingsComponent: input.settingsComponent,
                            group: input.group || "Custom",
                            icon: input.icon,
                            custom: true,
                        });
                    }

                    for (const display of load.default.displays || []) {
                        extensionRegistry.registerDisplayWidget({
                            component: display.component,
                            label: display.label,
                            supportedFieldTypes:
                                display.supportedFieldTypes || [],
                            preferredInputs: display.preferredInputs,
                            pluginSlug: enabledPlugin.name,
                            type: display.name,
                            settingsComponent: display.settingsComponent,
                            group: display.group || "Custom",
                            icon: display.icon,
                            custom: true,
                        });
                    }

                    for (const page of load.default.pages || []) {
                        extensionRegistry.registerNavItem({
                            component: page.component,
                            icon: page.icon,
                            label: page.label,
                            pluginSlug: enabledPlugin.name,
                            path: page.path,
                            sidebar: page.sidebar || true,
                        });
                    }
                } catch (e) {
                    console.error(e);
                }
            }
        });
        pluginsReady.value = true;
    }

    async function enablePlugin(name: string) {
        await client.plugins.enable(name, installParam());
        await fetchPlugins();
    }

    async function disablePlugin(name: string) {
        await client.plugins.disable(name, installParam());
        await fetchPlugins();
    }

    async function deletePlugin(name: string) {
        await client.plugins.delete(name, installParam());
        await fetchPlugins();
    }

    async function fetchPluginDetail(
        name: string,
        installId?: number | null,
    ): Promise<PluginStore> {
        activeInstallId.value = installId ?? null;
        pluginLoading.value = true;
        try {
            const response = await client.plugins.get(
                name,
                installId != null
                    ? { searchParams: { install_id: String(installId) } }
                    : undefined,
            );
            const detail = response?.data || response;
            currentPlugin.value = mapBackendPlugin(detail);
            return currentPlugin.value;
        } catch (e) {
            throw e;
        } finally {
            pluginLoading.value = false;
        }
    }

    async function fetchPluginSchema(
        name: string,
    ): Promise<PluginSchemaResponse> {
        const backendSchema = await client.plugins.schema(name, installParam());
        return mapSchema(backendSchema);
    }

    async function fetchPluginMigrations(
        name: string,
    ): Promise<{ migrations: SdkMigrationStatus[] }> {
        const response = await client.migrations.list(name, installParam());
        if (Array.isArray(response)) {
            return { migrations: response.map(mapMigration) };
        }
        const data =
            (response as { migrations?: BackendMigrationStatus[] })
                .migrations || [];
        return { migrations: data.map(mapMigration) };
    }

    async function runPendingMigrations(name: string): Promise<any> {
        return await client.migrations.run(name, installParam());
    }

    async function rollbackMigration(
        name: string,
        version: string,
    ): Promise<any> {
        return await client.migrations.rollback(name, version, installParam());
    }

    async function fetchPluginAssets(name: string): Promise<null | any> {
        const assets = await client.plugins.assets(name, installParam());
        console.log({ assets });
        if (!assets?.js) {
            return null;
        }
        const blob = new Blob([assets.js], {
            type: "application/javascript",
        });
        const url = URL.createObjectURL(blob);
        const module = await import(/* @vite-ignore */ url);
        URL.revokeObjectURL(url);

        const existingCSS = document.querySelector("style[cid='" + name + "']");

        if (existingCSS) existingCSS.remove();

        if (assets.css) {
            const style = document.createElement("style");
            style.innerHTML = assets.css;
            style.setAttribute("cid", name);
            document.head.appendChild(style);
        }
        return module;
    }

    async function fetchPluginPages(name: string): Promise<SdkPluginPage[]> {
        const pages = await client.plugins.pages(name, installParam());
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
        await client.settings.update(name, settings, installParam());
        if (!currentPlugin.value) {
            throw new Error(`Plugin "${name}" not found in store`);
        }
        return currentPlugin.value;
    }

    async function fetchPluginSettings(name: string): Promise<{
        settings: Record<string, unknown>;
        schema: Record<string, unknown> | null;
    }> {
        return (await client.settings.get(name, installParam())) as {
            settings: Record<string, unknown>;
            schema: Record<string, unknown> | null;
        };
    }

    async function fetchPluginDocs(name: string): Promise<{
        plugin: string;
        docs: Array<{ path: string; size: number }>;
    }> {
        const response = await client.plugins.docs(name, installParam());
        return response.data || { plugin: name, docs: [] };
    }

    async function fetchDocContent(
        name: string,
        docPath: string,
    ): Promise<string> {
        const content = await client.plugins.docContent(
            name,
            docPath,
            installParam(),
        );
        return content;
    }

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
        // `requestLogs` builds its own searchParams from the URLSearchParams we
        // pass, so the install id must be added here rather than via
        // `installParam` (whose `searchParams` would override the log filters).
        if (activeInstallId.value != null)
            params.set("install_id", String(activeInstallId.value));

        const response = await client.plugins.requestLogs(name, params);
        return response.data || { logs: [], next_cursor: null };
    }

    async function fetchPluginLogDetail(
        name: string,
        requestId: string,
    ): Promise<LogDetailResponse> {
        const response = await client.plugins.requestLogDetail(
            name,
            requestId,
            installParam(),
        );
        return response.data || { request: null as any, host_calls: [] };
    }

    async function fetchPluginDockerInfo(
        name: string,
    ): Promise<{ data?: DockerInfoResponse }> {
        const response = await client.plugins.runtimeInfo(name, installParam());
        return response as { data?: DockerInfoResponse };
    }

    async function fetchPluginVersions(
        name: string,
    ): Promise<ListVersionsResponse> {
        const response = await client.plugins.versions(name, installParam());
        return response.data || { versions: [] };
    }

    async function deployPluginVersion(
        name: string,
        tag: string,
    ): Promise<{ data?: { status: string } }> {
        const response = await client.plugins.deploy(
            name,
            tag,
            undefined,
            installParam(),
        );
        return response as { data?: { status: string } };
    }

    async function fetchPluginInstances(slug: string): Promise<InstanceInfo[]> {
        const json = await client.plugins.instances(slug, installParam());
        return json.data?.instances || [];
    }

    async function fetchInstanceDetail(
        slug: string,
        taskId: string,
    ): Promise<InstanceDetail> {
        const json = await client.plugins.instance(
            slug,
            taskId,
            installParam(),
        );
        return json.data;
    }

    async function fetchInstanceStats(
        slug: string,
        taskId: string,
    ): Promise<DeploymentStatsSnapshot> {
        const json = await client.plugins.instanceStats(
            slug,
            taskId,
            installParam(),
        );
        return json.data;
    }

    async function fetchInstanceLogs(
        slug: string,
        taskId: string,
    ): Promise<string[]> {
        return (await client.plugins.instanceLogs(
            slug,
            taskId,
            installParam(),
        )) as string[];
    }

    async function scalePlugin(
        slug: string,
        replicas: number,
        resourceLimits?: { cpu_limit: number; memory_limit: number },
    ): Promise<void> {
        await client.plugins.scale(
            slug,
            {
                replicas,
                resource_limits: resourceLimits,
            },
            installParam(),
        );
    }

    async function restartPlugin(
        name: string,
        containerId?: string,
    ): Promise<any> {
        return await client.plugins.restart(name, containerId, installParam());
    }

    async function fetchPluginScopes(slug: string): Promise<ScopesResponse> {
        const json = await client.plugins.scopes(slug, installParam());
        return json.data || { requested_scopes: [], granted_scopes: [] };
    }

    async function updatePluginScopes(
        slug: string,
        scopes: string[],
    ): Promise<void> {
        await client.plugins.updateScopes(slug, scopes, installParam());
    }

    return {
        plugins,
        loading,
        error,
        activeInstallId,
        pluginsReady,
        totalPlugins,
        enabledPlugins,
        disabledPlugins,
        systemPlugins,
        userPlugins,
        globalPlugins,
        fetchPlugins,
        resetRegistrations,
        enablePlugin,
        disablePlugin,
        deletePlugin,
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
        restartPlugin,
    };
});

/**
 * Register the built-in system view types (Table, Cards, Kanban) in the
 * extension registry. Must run on app start regardless of whether the user
 * can list/load plugins — otherwise users without the `plugins.read` scope
 * (e.g. policy-restricted roles) get no renderable data views.
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
