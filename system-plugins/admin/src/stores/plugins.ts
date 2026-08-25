import { useExtensionRegistryStore } from "./extensionRegistry";
import { defineStore } from "pinia";
import { ref, computed, nextTick } from "vue";
import type {
    Plugin,
    PluginSchemaResponse,
    MigrationStatus as SdkMigrationStatus,
    PluginPage as SdkPluginPage,
} from "alcedocore-sdk-node";
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
} from "alcedocore-sdk-node";

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
    container_id: string | null;
    container_state: string | null;
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
    registry?: string | null;
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
        pluginLoading = ref(false);

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

    async function fetchPlugins() {
        await withAsyncHandlingVoid(loading, error, async () => {
            const response = await client.plugins.list();
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

            extensionRegistry.registerViewType({
                component: TableView,
                label: "Table",
                pluginSlug: "system",
                type: "table",
                settingsComponent: TableViewSettings,
            });

            extensionRegistry.registerViewType({
                component: CardsView,
                label: "Cards",
                pluginSlug: "system",
                type: "cards",
                settingsComponent: CardsViewSettings,
            });

            extensionRegistry.registerViewType({
                component: KanbanView,
                label: "Kanban",
                pluginSlug: "system",
                type: "kanban",
                settingsComponent: KanbanViewSettings,
            });
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

    async function deletePlugin(name: string) {
        await client.plugins.delete(name);
        await fetchPlugins();
    }

    async function fetchPluginDetail(name: string): Promise<PluginStore> {
        pluginLoading.value = true;
        try {
            const response = await client.plugins.get(name);
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

    async function fetchPluginAssets(name: string): Promise<null | any> {
        const assets = await client.plugins.assets(name);
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

    async function fetchPluginDocs(name: string): Promise<{
        plugin: string;
        docs: Array<{ path: string; size: number }>;
    }> {
        const response = await client.plugins.docs(name);
        return response.data || { plugin: name, docs: [] };
    }

    async function fetchDocContent(
        name: string,
        docPath: string,
    ): Promise<string> {
        const content = await client.plugins.docContent(name, docPath);
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

        const response = await client.plugins.requestLogs(name, params);
        return response.data || { logs: [], next_cursor: null };
    }

    async function fetchPluginLogDetail(
        name: string,
        requestId: string,
    ): Promise<LogDetailResponse> {
        const response = await client.plugins.requestLogDetail(name, requestId);
        return response.data || { request: null as any, host_calls: [] };
    }

    async function fetchPluginDockerInfo(
        name: string,
    ): Promise<{ data?: DockerInfoResponse }> {
        const response = await client.plugins.runtimeInfo(name);
        return response as { data?: DockerInfoResponse };
    }

    async function fetchPluginVersions(
        name: string,
    ): Promise<ListVersionsResponse> {
        const response = await client.plugins.versions(name);
        return response.data || { versions: [] };
    }

    async function deployPluginVersion(
        name: string,
        tag: string,
    ): Promise<{ data?: { status: string } }> {
        const response = await client.plugins.deploy(name, tag);
        return response as { data?: { status: string } };
    }

    async function fetchPluginInstances(slug: string): Promise<InstanceInfo[]> {
        const json = await client.plugins.instances(slug);
        return json.data?.instances || [];
    }

    async function fetchInstanceDetail(
        slug: string,
        taskId: string,
    ): Promise<InstanceDetail> {
        const json = await client.plugins.instance(slug, taskId);
        return json.data;
    }

    async function fetchInstanceStats(
        slug: string,
        taskId: string,
    ): Promise<ContainerStatsSnapshot> {
        const json = await client.plugins.instanceStats(slug, taskId);
        return json.data;
    }

    async function fetchInstanceLogs(
        slug: string,
        taskId: string,
    ): Promise<string[]> {
        return (await client.plugins.instanceLogs(slug, taskId)) as string[];
    }

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

    async function fetchPluginScopes(slug: string): Promise<ScopesResponse> {
        const json = await client.plugins.scopes(slug);
        return json.data || { requested_scopes: [], granted_scopes: [] };
    }

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
        totalPlugins,
        enabledPlugins,
        disabledPlugins,
        systemPlugins,
        userPlugins,
        fetchPlugins,
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
