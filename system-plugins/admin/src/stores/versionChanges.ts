import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { AlcedoApiError } from "@alcedocore/sdk";
import { useAlcedoClient } from "../composables/useAlcedoClient";

export const PRODUCTION_VERSION = "production";

export interface VersionRow {
    id: number;
    version_name: string;
    /** App api_names already deployed from this version (from the core's
     * `version_deployed` markers) — used to hide deployed `app_created` edits. */
    deployed_apps?: string[];
}

export interface AppVersionRow {
    id: number;
    app_id: number;
    app_name: string;
    api_name: string;
    version_id: number;
    version_name: string;
}

export type ChangeSource = "system" | "collection" | "app_created";

/** One selectable change (edit) of a version, normalized from the log rows. */
export interface VersionChange {
    /** UI key — `${source}:${editId}`. */
    id: string;
    /**
     * Raw edit id (log row id, or the app api_name for app_created edits).
     * Both are sent in `edit_ids` — see {@link editIdsOf}.
     */
    editId: number | string;
    source: ChangeSource;
    app: string;
    action: string;
    target: string | null;
    item_id: string | null;
    actor_id: string | null;
    description: string | null;
    diff: Record<string, unknown> | null;
    metadata: Record<string, unknown> | null;
    created_at: string;
}

export type DeployOnConflict = "fail" | "skip" | "overwrite";

export interface DeployRequestBody {
    /** Selected edit ids — numeric log-row ids plus `app_created` app
     * api_names; `null` deploys all edits of the version. */
    edit_ids: Array<number | string> | null;
    dry_run: boolean;
    on_conflict: DeployOnConflict;
}

export interface DeployConflict {
    type?: string;
    app?: string;
    collection?: string;
    item_id?: string;
    detail?: string;
    [key: string]: unknown;
}

export interface DeployResult {
    ok: boolean;
    httpStatus: number;
    deployed: boolean;
    dryRun: boolean;
    conflicts: DeployConflict[];
    /** Field changes a deploy cannot apply (type change / deletion on an
     * existing collection) — refused pre-write, even with overwrite. */
    unsupported: DeployConflict[];
    edits: unknown[];
    message: string | null;
}

const LOG_PAGE_LIMIT = 1000;

// ── Pure helpers (exported for tests) ──

export function changeId(
    source: ChangeSource,
    editId: number | string,
): string {
    return `${source}:${editId}`;
}

export function normalizeSystemLog(row: any, app: string): VersionChange {
    const editId = row.id as number | string;
    return {
        id: changeId("system", editId),
        editId,
        source: "system",
        app,
        action: row.action ?? "",
        target: row.target ?? null,
        item_id: null,
        actor_id: row.actor_id ?? null,
        description: row.description ?? null,
        diff: null,
        metadata: row.metadata ?? null,
        created_at: row.created_at ?? "",
    };
}

export function normalizeCollectionLog(row: any, app: string): VersionChange {
    const editId = row.id as number | string;
    return {
        id: changeId("collection", editId),
        editId,
        source: "collection",
        app,
        action: row.action ?? "",
        target: row.collection_name ?? null,
        item_id: row.item_id ?? null,
        actor_id: row.actor_id ?? null,
        description: row.description ?? null,
        diff: row.diff ?? null,
        metadata: row.metadata ?? null,
        created_at: row.created_at ?? "",
    };
}

/** Synthetic edit for an app that exists in the version but not in production. */
export function appCreatedChange(row: AppVersionRow): VersionChange {
    return {
        id: changeId("app_created", row.api_name),
        editId: row.api_name,
        source: "app_created",
        app: row.api_name,
        action: "app_created",
        target: row.api_name,
        item_id: null,
        actor_id: null,
        description: `App '${row.app_name}' was created in version '${row.version_name}'`,
        diff: null,
        metadata: {
            app_id: row.app_id,
            version_id: row.version_id,
            version_name: row.version_name,
        },
        created_at: "",
    };
}

/** Apps linked to `versionName` that are not linked to production and were
 * not already deployed from this version (`deployedApps`). */
export function findAppCreatedApps(
    appVersions: AppVersionRow[],
    versionName: string,
    productionName: string = PRODUCTION_VERSION,
    deployedApps: string[] = [],
): AppVersionRow[] {
    const prodAppIds = new Set(
        appVersions
            .filter((a) => a.version_name === productionName)
            .map((a) => a.app_id),
    );
    const deployed = new Set(deployedApps);
    return appVersions.filter(
        (a) =>
            a.version_name === versionName &&
            !prodAppIds.has(a.app_id) &&
            !deployed.has(a.api_name),
    );
}

export function deployableVersionsForApp(
    versions: VersionRow[],
    appVersions: AppVersionRow[],
    app: string,
    productionName: string = PRODUCTION_VERSION,
): VersionRow[] {
    const linkedIds = new Set(
        appVersions.filter((a) => a.api_name === app).map((a) => a.version_id),
    );
    return versions.filter(
        (v) => v.version_name !== productionName && linkedIds.has(v.id),
    );
}

/**
 * Numeric edit ids of the given changes. Non-numeric ids (e.g. the
 * `app_created` api_name) are filtered out — kept for callers that only
 * want log-row ids.
 */
export function numericEditIds(changes: VersionChange[]): number[] {
    return changes
        .map((c) => c.editId)
        .filter((id): id is number => typeof id === "number");
}

/**
 * Edit ids of the given changes as sent in `edit_ids`: numeric log-row ids
 * plus string ids (`app_created` app api_names — the core selects new-app
 * edits by api_name in selected mode).
 */
export function editIdsOf(changes: VersionChange[]): Array<number | string> {
    return changes.map((c) => c.editId);
}

export function buildDeployBody(
    editIds: Array<number | string> | null,
    dryRun: boolean,
    onConflict: DeployOnConflict = "fail",
): DeployRequestBody {
    return { edit_ids: editIds, dry_run: dryRun, on_conflict: onConflict };
}

/** Human-readable reason for one conflict entry of `data.conflicts`. */
export function conflictReason(c: DeployConflict): string {
    if (c?.detail) return String(c.detail);
    const parts: string[] = [];
    if (c?.app) parts.push(`app '${c.app}'`);
    if (c?.collection) parts.push(`collection '${c.collection}'`);
    if (c?.item_id) parts.push(`item ${c.item_id}`);
    const label = String(c?.type ?? "conflict").replace(/_/g, " ");
    return parts.length ? `${label}: ${parts.join(", ")}` : label;
}

/**
 * Every reason a deploy result cannot be applied as-is: conflicts
 * (`data.conflicts`) plus unsupported field changes (`data.unsupported`),
 * as human-readable strings for the check popup.
 */
export function deployIssueReasons(res: DeployResult): string[] {
    return [
        ...res.conflicts.map(conflictReason),
        ...res.unsupported.map(conflictReason),
    ];
}

/** True when the result has no blocking issues at all (deployable as-is). */
export function deployHasNoIssues(res: DeployResult): boolean {
    return res.conflicts.length === 0 && res.unsupported.length === 0;
}

/** True when the only issues are conflicts (deployable with overwrite). */
export function deployHasOnlyConflicts(res: DeployResult): boolean {
    return res.conflicts.length > 0 && res.unsupported.length === 0;
}

/** Newest first; entries without a timestamp (app_created) go last. */
export function sortChanges(changes: VersionChange[]): VersionChange[] {
    return [...changes].sort((a, b) => {
        if (!a.created_at) return 1;
        if (!b.created_at) return -1;
        return b.created_at.localeCompare(a.created_at);
    });
}

// ── Grouping / display ──

export interface ChangeGroup {
    key: string;
    label: string;
    changes: VersionChange[];
}

const GROUP_ORDER: { key: string; label: string }[] = [
    { key: "apps", label: "Apps" },
    { key: "collections", label: "Collections" },
    { key: "fields", label: "Fields" },
    { key: "layouts", label: "Layouts & views" },
    { key: "menus", label: "Menus" },
    { key: "settings", label: "App settings" },
    { key: "files", label: "Files" },
    { key: "items", label: "Items" },
    { key: "permissions", label: "Permissions (not deployed)" },
    { key: "other", label: "Other system changes" },
];

export interface UserRow {
    id: string;
    email: string;
    display_name: string | null;
}

/** Actor label: the user's display name or email, else the raw actor id. */
export function actorLabel(c: VersionChange, users: UserRow[]): string {
    if (!c.actor_id) return "—";
    const u = users.find((x) => x.id === c.actor_id);
    if (u) return u.display_name || u.email;
    return c.actor_id;
}

/** The change-type group a change belongs to (one table per group in the UI). */
export function changeGroup(c: VersionChange): { key: string; label: string } {
    if (c.source === "app_created") return GROUP_ORDER[0];
    if (c.source === "collection") return { key: "items", label: "Items" };
    switch (c.target) {
        case "alcedocore_collections":
            return { key: "collections", label: "Collections" };
        case "alcedocore_fields":
            return { key: "fields", label: "Fields" };
        case "alcedocore_collection_layouts":
        case "alcedocore_collection_layout_roles":
        case "alcedocore_collection_sections":
        case "alcedocore_collection_views":
        case "alcedocore_collection_view_fields":
            return { key: "layouts", label: "Layouts & views" };
        case "alcedocore_menus":
        case "alcedocore_menu_sections":
        case "alcedocore_menu_items":
        case "alcedocore_menu_roles":
            return { key: "menus", label: "Menus" };
        case "alcedocore_app_settings":
            return { key: "settings", label: "App settings" };
        case "alcedocore_file_folders":
        case "alcedocore_file_metadata":
        case "alcedocore_item_files":
            return { key: "files", label: "Files" };
        case "alcedocore_roles":
        case "alcedocore_role_scopes":
        case "alcedocore_role_policies":
        case "alcedocore_policies":
        case "alcedocore_policy_permissions":
        case "alcedocore_user_roles":
            return { key: "permissions", label: "Permissions (not deployed)" };
        default:
            return { key: "other", label: "Other system changes" };
    }
}

/** Groups the changes into the fixed group order, skipping empty groups. */
export function groupChanges(changes: VersionChange[]): ChangeGroup[] {
    const byKey = new Map<string, ChangeGroup>();
    for (const c of changes) {
        const g = changeGroup(c);
        const existing = byKey.get(g.key);
        if (existing) existing.changes.push(c);
        else byKey.set(g.key, { key: g.key, label: g.label, changes: [c] });
    }
    return GROUP_ORDER.map((g) => byKey.get(g.key)).filter(
        (g): g is ChangeGroup => Boolean(g),
    );
}

/** Friendly subject of a change: the collection/field/app name from metadata. */
export function changeSubject(c: VersionChange): string {
    if (c.source === "app_created") return c.app;
    const m = (c.metadata ?? {}) as Record<string, unknown>;
    const name = m.name ?? m.api_name ?? m.display_name ?? m.table;
    if (name != null && name !== "") return String(name);
    if (c.source === "collection")
        return `${c.target ?? "?"} #${c.item_id ?? "?"}`;
    return c.target ?? "—";
}

/**
 * Collection name for a field change (the Fields table's "Collection"
 * column): just the name, falling back to `#<id>`.
 */
export function fieldCollection(
    c: VersionChange,
    collectionName?: (app: string, id: number) => string | undefined,
): string {
    const m = (c.metadata ?? {}) as Record<string, unknown>;
    const id = Number(m.collection_id);
    if (!id) return "—";
    return collectionName?.(c.app, id) ?? `#${id}`;
}

/**
 * Extra detail line for a change: a field's collection, a collection's
 * table, an item id, … `collectionName` resolves `collection_id` to a name.
 */
export function changeDetail(
    c: VersionChange,
    collectionName?: (app: string, id: number) => string | undefined,
): string {
    const m = (c.metadata ?? {}) as Record<string, unknown>;
    if (c.source === "collection") return `item ${c.item_id ?? "—"}`;
    if (c.target === "alcedocore_fields" && m.collection_id != null) {
        const id = Number(m.collection_id);
        const name = collectionName?.(c.app, id);
        return name ? `in collection '${name}'` : `in collection #${id}`;
    }
    if (c.target === "alcedocore_collections" && m.table)
        return `table '${String(m.table)}'`;
    return c.item_id ?? c.target ?? "—";
}

// ── Store ──

export const useVersionChangesStore = defineStore("versionChanges", () => {
    const { client } = useAlcedoClient();

    const versions = ref<VersionRow[]>([]);
    const appVersions = ref<AppVersionRow[]>([]);
    const users = ref<UserRow[]>([]);
    const changes = ref<VersionChange[]>([]);
    /** app api_name → collection_id → collection name (for field details). */
    const collectionNames = ref<Record<string, Record<number, string>>>({});
    const selectedIds = ref<string[]>([]);
    const loading = ref(false);
    const error = ref<string | null>(null);
    const deploying = ref(false);
    const lastResult = ref<DeployResult | null>(null);

    const selectedChanges = computed(() =>
        changes.value.filter((c) => selectedIds.value.includes(c.id)),
    );
    /**
     * `edit_ids` for the current selection: numeric log-row ids plus
     * `app_created` app api_names (`null` when nothing is selected =
     * deploy all). In selected mode the core only deploys what is listed,
     * so app api_names must be included or a new app would never deploy.
     */
    const selectedEditIds = computed<Array<number | string> | null>(() =>
        selectedIds.value.length === 0
            ? null
            : editIdsOf(selectedChanges.value),
    );
    const allSelected = computed(
        () =>
            changes.value.length > 0 &&
            selectedIds.value.length === changes.value.length,
    );

    /** Changes grouped by change type — one table per group in the UI. */
    const groupedChanges = computed(() => groupChanges(changes.value));

    function collectionName(app: string, id: number): string | undefined {
        return collectionNames.value[app]?.[id];
    }

    /** Loads `{id, name}` of the app's collections in this version. */
    async function fetchCollectionNames(app: string, version: string) {
        try {
            const res = (await client.items.list(
                "alcedocore_collections",
                { fields: "id,name", limit: "1000" },
                { app, version },
            )) as { data?: Array<{ id: number; name: string }> };
            const map: Record<number, string> = {};
            for (const row of res?.data ?? []) map[row.id] = row.name;
            collectionNames.value = { ...collectionNames.value, [app]: map };
        } catch {
            // Without names the UI falls back to `collection #<id>`.
        }
    }

    async function fetchVersions() {
        versions.value = await client.versions.list();
    }

    async function fetchAppVersions() {
        appVersions.value = await client.apps.appVersions();
    }

    /** Platform users, to resolve log `actor_id` UUIDs to a name/email. */
    async function fetchUsers() {
        users.value = await client.users.list();
    }

    /** Pending system-log edits of one app × version.
     *
     * User-item (collection-log) edits are never deployable, so they are not
     * listed as changes at all (they stay visible in the activity logs).
     * Edits already applied by a previous deploy of this version are hidden
     * via `exclude_deployed`. */
    async function fetchChangesForApp(
        app: string,
        version: string,
    ): Promise<VersionChange[]> {
        const opts = { app, version };
        const out: VersionChange[] = [];
        let offset = 0;
        for (;;) {
            const page = (await client.activityLogs.listAppSystem(
                { excludeDeployed: true, limit: LOG_PAGE_LIMIT, offset },
                opts,
            )) as { data?: any[]; total?: number };
            const rows = page?.data ?? [];
            for (const row of rows) out.push(normalizeSystemLog(row, app));
            const total = page?.total ?? rows.length;
            offset += rows.length;
            if (rows.length === 0 || offset >= total) break;
        }
        return out;
    }

    /** Global scope: changes of every app linked to the version + app_created edits. */
    async function loadVersionChanges(version: VersionRow) {
        loading.value = true;
        error.value = null;
        changes.value = [];
        selectedIds.value = [];
        try {
            const linked = appVersions.value.filter(
                (a) => a.version_id === version.id,
            );
            const perApp = await Promise.all(
                linked.map(async (a) => {
                    try {
                        await fetchCollectionNames(
                            a.api_name,
                            version.version_name,
                        );
                        return await fetchChangesForApp(
                            a.api_name,
                            version.version_name,
                        );
                    } catch {
                        // An app without a schema in this version has no logs.
                        return [] as VersionChange[];
                    }
                }),
            );
            const flat = perApp.flat();
            for (const row of findAppCreatedApps(
                appVersions.value,
                version.version_name,
                PRODUCTION_VERSION,
                version.deployed_apps ?? [],
            )) {
                flat.push(appCreatedChange(row));
            }
            changes.value = sortChanges(flat);
        } catch (e) {
            error.value =
                e instanceof Error ? e.message : "Failed to load changes";
        } finally {
            loading.value = false;
        }
    }

    /** App zone: changes of the single app × version from the URL context. */
    async function loadAppChanges(app: string, versionName: string) {
        loading.value = true;
        error.value = null;
        changes.value = [];
        selectedIds.value = [];
        try {
            await fetchCollectionNames(app, versionName);
            const flat = await fetchChangesForApp(app, versionName);
            const versionRow = versions.value.find(
                (v) => v.version_name === versionName,
            );
            const created = findAppCreatedApps(
                appVersions.value,
                versionName,
                PRODUCTION_VERSION,
                versionRow?.deployed_apps ?? [],
            ).find((a) => a.api_name === app);
            if (created) flat.push(appCreatedChange(created));
            changes.value = sortChanges(flat);
        } catch (e) {
            error.value =
                e instanceof Error ? e.message : "Failed to load changes";
        } finally {
            loading.value = false;
        }
    }

    function toggleSelect(id: string) {
        const i = selectedIds.value.indexOf(id);
        if (i >= 0) selectedIds.value.splice(i, 1);
        else selectedIds.value.push(id);
    }

    function toggleSelectAll() {
        if (allSelected.value) selectedIds.value = [];
        else selectedIds.value = changes.value.map((c) => c.id);
    }

    function clearSelection() {
        selectedIds.value = [];
    }

    /**
     * POST /api/platform/versions/{id}/deploy via the SDK client. On success
     * the JSend `data` is unwrapped; on a JSend `fail` (e.g. HTTP 409 deploy
     * conflicts) the SDK raises `AlcedoApiError`, whose `data` payload carries
     * `data.conflicts` for the conflict popup.
     */
    async function deploy(
        versionId: number,
        body: DeployRequestBody,
    ): Promise<DeployResult> {
        deploying.value = true;
        try {
            // Deploys clone a backup of production first and can take well
            // over the 30s default client timeout — give them 5 minutes.
            const data = (await client.versions.deploy(versionId, body, {
                timeout: 300_000,
            })) as {
                deployed?: boolean;
                conflicts?: DeployConflict[];
                unsupported?: DeployConflict[];
                edits?: unknown[];
            } | null;
            const result: DeployResult = {
                ok: true,
                httpStatus: 200,
                deployed: Boolean(data?.deployed),
                dryRun: body.dry_run,
                conflicts: Array.isArray(data?.conflicts) ? data.conflicts : [],
                unsupported: Array.isArray(data?.unsupported)
                    ? data.unsupported
                    : [],
                edits: Array.isArray(data?.edits) ? data.edits : [],
                message: null,
            };
            lastResult.value = result;
            return result;
        } catch (e) {
            if (e instanceof AlcedoApiError) {
                const data = (e.data ?? null) as {
                    deployed?: boolean;
                    conflicts?: DeployConflict[];
                    unsupported?: DeployConflict[];
                    edits?: unknown[];
                } | null;
                const result: DeployResult = {
                    ok: false,
                    httpStatus: e.httpStatus ?? 0,
                    deployed: Boolean(data?.deployed),
                    dryRun: body.dry_run,
                    conflicts: Array.isArray(data?.conflicts)
                        ? data.conflicts
                        : [],
                    unsupported: Array.isArray(data?.unsupported)
                        ? data.unsupported
                        : [],
                    edits: Array.isArray(data?.edits) ? data.edits : [],
                    message: e.message || "The changes could not be applied",
                };
                lastResult.value = result;
                return result;
            }
            const result: DeployResult = {
                ok: false,
                httpStatus: 0,
                deployed: false,
                dryRun: body.dry_run,
                conflicts: [],
                unsupported: [],
                edits: [],
                message:
                    e instanceof Error ? e.message : "Deploy request failed",
            };
            lastResult.value = result;
            return result;
        } finally {
            deploying.value = false;
        }
    }

    return {
        versions,
        appVersions,
        users,
        changes,
        collectionNames,
        selectedIds,
        loading,
        error,
        deploying,
        lastResult,
        selectedChanges,
        selectedEditIds,
        allSelected,
        groupedChanges,
        collectionName,
        fetchVersions,
        fetchAppVersions,
        fetchUsers,
        fetchChangesForApp,
        fetchCollectionNames,
        loadVersionChanges,
        loadAppChanges,
        toggleSelect,
        toggleSelectAll,
        clearSelection,
        deploy,
    };
});
