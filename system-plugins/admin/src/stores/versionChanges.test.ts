import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { AlcedoApiError } from "@alcedocore/sdk";
import {
    useVersionChangesStore,
    normalizeSystemLog,
    normalizeCollectionLog,
    appCreatedChange,
    findAppCreatedApps,
    deployableVersionsForApp,
    numericEditIds,
    editIdsOf,
    buildDeployBody,
    conflictReason,
    deployIssueReasons,
    deployHasNoIssues,
    deployHasOnlyConflicts,
    sortChanges,
    groupChanges,
    changeSubject,
    changeDetail,
    fieldCollection,
    actorLabel,
    type AppVersionRow,
    type DeployResult,
} from "./versionChanges";

const mocks = vi.hoisted(() => ({
    versionsList: vi.fn(),
    versionsDeploy: vi.fn(),
    appVersions: vi.fn(),
    usersList: vi.fn(),
    listAppSystem: vi.fn(),
    listCollections: vi.fn(),
    itemsList: vi.fn(),
}));

vi.mock("../composables/useAlcedoClient", () => ({
    useAlcedoClient: () => ({
        client: {
            versions: {
                list: mocks.versionsList,
                deploy: mocks.versionsDeploy,
            },
            apps: { appVersions: mocks.appVersions },
            users: { list: mocks.usersList },
            activityLogs: {
                listAppSystem: mocks.listAppSystem,
                listCollections: mocks.listCollections,
            },
            items: { list: mocks.itemsList },
        },
        isConnected: { value: true },
    }),
}));

const versions = [
    { id: 1, version_name: "production" },
    { id: 2, version_name: "staging" },
];

const appVersions: AppVersionRow[] = [
    { id: 1, app_id: 10, app_name: "CRM", api_name: "crm", version_id: 1, version_name: "production" },
    { id: 2, app_id: 10, app_name: "CRM", api_name: "crm", version_id: 2, version_name: "staging" },
    { id: 3, app_id: 11, app_name: "Helpdesk", api_name: "helpdesk", version_id: 2, version_name: "staging" },
];

const systemLogRow = {
    id: 12,
    action: "collection_created",
    actor_id: "user-1",
    created_at: "2025-01-02T10:00:00Z",
    description: "Created collection invoices",
    metadata: { foo: "bar" },
    request_id: "req-1",
    target: "invoices",
};

const collectionLogRow = {
    id: 13,
    action: "item_updated",
    actor_id: "user-2",
    collection_name: "contacts",
    item_id: "item-9",
    request_id: "req-2",
    description: null,
    metadata: null,
    diff: { name: { old: "A", new: "B" } },
    created_at: "2025-01-03T10:00:00Z",
};

beforeEach(() => {
    setActivePinia(createPinia());
    mocks.versionsList.mockResolvedValue(versions);
    mocks.appVersions.mockResolvedValue(appVersions);
    mocks.listAppSystem.mockImplementation(async (_params: any, opts: any) => {
        if (opts?.app === "crm") {
            return { data: [systemLogRow], total: 1, limit: 1000, offset: 0 };
        }
        return { data: [], total: 0, limit: 1000, offset: 0 };
    });
    mocks.listCollections.mockImplementation(async (_params: any, opts: any) => {
        if (opts?.app === "crm") {
            return { data: [collectionLogRow], total: 1, limit: 1000, offset: 0 };
        }
        return { data: [], total: 0, limit: 1000, offset: 0 };
    });
});

afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
});

describe("pure helpers", () => {
    it("normalizeSystemLog maps a system log row", () => {
        const c = normalizeSystemLog(systemLogRow, "crm");
        expect(c.id).toBe("system:12");
        expect(c.editId).toBe(12);
        expect(c.source).toBe("system");
        expect(c.app).toBe("crm");
        expect(c.action).toBe("collection_created");
        expect(c.target).toBe("invoices");
        expect(c.item_id).toBeNull();
        expect(c.diff).toBeNull();
        expect(c.created_at).toBe("2025-01-02T10:00:00Z");
    });

    it("normalizeCollectionLog maps a collection log row incl. diff", () => {
        const c = normalizeCollectionLog(collectionLogRow, "crm");
        expect(c.id).toBe("collection:13");
        expect(c.editId).toBe(13);
        expect(c.source).toBe("collection");
        expect(c.target).toBe("contacts");
        expect(c.item_id).toBe("item-9");
        expect(c.diff).toEqual({ name: { old: "A", new: "B" } });
    });

    it("appCreatedChange builds a selectable app_created edit", () => {
        const c = appCreatedChange(appVersions[2]);
        expect(c.id).toBe("app_created:helpdesk");
        expect(c.action).toBe("app_created");
        expect(c.app).toBe("helpdesk");
        expect(c.target).toBe("helpdesk");
    });

    it("numericEditIds keeps only numeric ids", () => {
        const changes = [
            normalizeSystemLog(systemLogRow, "crm"),
            normalizeCollectionLog(collectionLogRow, "crm"),
            appCreatedChange(appVersions[2]),
        ];
        expect(numericEditIds(changes)).toEqual([12, 13]);
        expect(numericEditIds([appCreatedChange(appVersions[2])])).toEqual([]);
    });

    it("editIdsOf keeps numeric ids AND app_created api_names (selected mode needs both)", () => {
        const changes = [
            normalizeSystemLog(systemLogRow, "crm"),
            appCreatedChange(appVersions[2]),
        ];
        expect(editIdsOf(changes)).toEqual([12, "helpdesk"]);
        // a selection of only a new app must still send its api_name —
        // edit_ids: [] would select nothing in the core
        expect(editIdsOf([appCreatedChange(appVersions[2])])).toEqual(["helpdesk"]);
    });

    it("findAppCreatedApps returns apps of the version missing in production", () => {
        const created = findAppCreatedApps(appVersions, "staging");
        expect(created.map((a) => a.api_name)).toEqual(["helpdesk"]);
        expect(findAppCreatedApps(appVersions, "production")).toEqual([]);
    });

    it("deployableVersionsForApp returns non-production versions with the app linked", () => {
        // crm is linked to production (1) and staging (2): only staging is a
        // deploy source; production is excluded even though it is linked.
        expect(
            deployableVersionsForApp(versions, appVersions, "crm").map(
                (v) => v.version_name,
            ),
        ).toEqual(["staging"]);
        expect(
            deployableVersionsForApp(versions, appVersions, "helpdesk").map(
                (v) => v.version_name,
            ),
        ).toEqual(["staging"]);
        // An app linked to no non-production version has no deploy source.
        expect(deployableVersionsForApp(versions, appVersions, "unknown")).toEqual(
            [],
        );
    });

    it("findAppCreatedApps hides apps already deployed from the version", () => {
        expect(
            findAppCreatedApps(appVersions, "staging", "production", ["helpdesk"]),
        ).toEqual([]);
    });

    it("buildDeployBody builds the deploy payload with numeric edit_ids only", () => {
        expect(buildDeployBody(null, false, "fail")).toEqual({
            edit_ids: null,
            dry_run: false,
            on_conflict: "fail",
        });
        expect(buildDeployBody([12, 13], true)).toEqual({
            edit_ids: [12, 13],
            dry_run: true,
            on_conflict: "fail",
        });
    });

    it("conflictReason prefers detail, else composes from fields", () => {
        expect(
            conflictReason({
                type: "relation_missing_in_prod",
                app: "crm",
                detail: "collection 'invoices' does not exist in crm010production",
            }),
        ).toBe("collection 'invoices' does not exist in crm010production");
        expect(
            conflictReason({
                type: "changed_on_both_sides",
                app: "crm",
                collection: "contacts",
                item_id: "item-9",
            }),
        ).toBe("changed on both sides: app 'crm', collection 'contacts', item item-9");
        expect(conflictReason({})).toBe("conflict");
    });

    const baseResult = (over: Partial<DeployResult>): DeployResult => ({
        ok: true,
        httpStatus: 200,
        deployed: false,
        dryRun: true,
        conflicts: [],
        unsupported: [],
        edits: [],
        message: null,
        ...over,
    });

    it("deployIssueReasons lists conflicts AND unsupported entries", () => {
        const res = baseResult({
            conflicts: [
                {
                    type: "relation_missing_in_prod",
                    app: "crm",
                    detail: "collection 'invoices' does not exist in crm010production",
                },
            ],
            unsupported: [
                {
                    type: "field_type_change",
                    app: "crm",
                    collection: "contacts",
                    detail: "field 'email' changed type from string to integer",
                },
            ],
        });
        expect(deployIssueReasons(res)).toEqual([
            "collection 'invoices' does not exist in crm010production",
            "field 'email' changed type from string to integer",
        ]);
        expect(deployHasNoIssues(res)).toBe(false);
        expect(deployHasOnlyConflicts(res)).toBe(false);
    });

    it("deployHasNoIssues / deployHasOnlyConflicts classify the check result", () => {
        expect(deployHasNoIssues(baseResult({}))).toBe(true);
        expect(deployHasOnlyConflicts(baseResult({}))).toBe(false);
        const conflictsOnly = baseResult({
            conflicts: [{ type: "changed_on_both_sides", app: "crm" }],
        });
        expect(deployHasNoIssues(conflictsOnly)).toBe(false);
        expect(deployHasOnlyConflicts(conflictsOnly)).toBe(true);
        const unsupportedOnly = baseResult({
            unsupported: [{ type: "field_deleted", app: "crm" }],
        });
        // unsupported can never be overwritten
        expect(deployHasOnlyConflicts(unsupportedOnly)).toBe(false);
    });

    it("sortChanges sorts newest first and undated last", () => {
        const sorted = sortChanges([
            normalizeSystemLog(systemLogRow, "crm"),
            appCreatedChange(appVersions[2]),
            normalizeCollectionLog(collectionLogRow, "crm"),
        ]);
        expect(sorted.map((c) => c.id)).toEqual([
            "collection:13",
            "system:12",
            "app_created:helpdesk",
        ]);
    });

    it("groupChanges groups by change type in fixed order, skipping empty groups", () => {
        const fieldRow = normalizeSystemLog(
            {
                id: 20,
                action: "item_created",
                actor_id: "user-1",
                created_at: "2025-01-04T10:00:00Z",
                description: null,
                metadata: { api_name: "title", collection_id: 12 },
                request_id: "req-3",
                target: "alcedocore_fields",
            },
            "crm",
        );
        const groups = groupChanges([
            normalizeCollectionLog(collectionLogRow, "crm"),
            fieldRow,
            normalizeSystemLog(
                { ...systemLogRow, target: "alcedocore_collections" },
                "crm",
            ),
            appCreatedChange(appVersions[2]),
        ]);
        expect(groups.map((g) => g.key)).toEqual([
            "apps",
            "collections",
            "fields",
            "items",
        ]);
        expect(groups.find((g) => g.key === "fields")?.changes).toHaveLength(1);
    });

    it("changeSubject prefers the metadata name (collection/field/app)", () => {
        expect(
            changeSubject(
                normalizeSystemLog(
                    { ...systemLogRow, metadata: { name: "release_notes", table: "release_notes" } },
                    "crm",
                ),
            ),
        ).toBe("release_notes");
        expect(
            changeSubject(
                normalizeSystemLog(
                    { ...systemLogRow, target: "alcedocore_fields", metadata: { api_name: "title", collection_id: 12 } },
                    "crm",
                ),
            ),
        ).toBe("title");
        expect(changeSubject(appCreatedChange(appVersions[2]))).toBe("helpdesk");
        expect(changeSubject(normalizeCollectionLog(collectionLogRow, "crm"))).toBe(
            "contacts #item-9",
        );
    });

    it("changeDetail shows the field's collection name when resolvable", () => {
        const field = normalizeSystemLog(
            { ...systemLogRow, target: "alcedocore_fields", metadata: { api_name: "title", collection_id: 12 } },
            "crm",
        );
        const names = (_app: string, id: number) => (id === 12 ? "release_notes" : undefined);
        expect(changeDetail(field, names)).toBe("in collection 'release_notes'");
        expect(changeDetail(field)).toBe("in collection #12");
        expect(
            changeDetail(
                normalizeSystemLog(
                    {
                        ...systemLogRow,
                        target: "alcedocore_collections",
                        metadata: { name: "release_notes", table: "release_notes" },
                    },
                    "crm",
                ),
            ),
        ).toBe("table 'release_notes'");
        expect(changeDetail(normalizeCollectionLog(collectionLogRow, "crm"))).toBe(
            "item item-9",
        );
    });

    it("fieldCollection returns just the collection name, falling back to #id", () => {
        const field = normalizeSystemLog(
            { ...systemLogRow, target: "alcedocore_fields", metadata: { api_name: "title", collection_id: 12 } },
            "crm",
        );
        const names = (_app: string, id: number) => (id === 12 ? "release_notes" : undefined);
        expect(fieldCollection(field, names)).toBe("release_notes");
        expect(fieldCollection(field)).toBe("#12");
        const noMeta = normalizeSystemLog(
            { ...systemLogRow, target: "alcedocore_fields", metadata: null },
            "crm",
        );
        expect(fieldCollection(noMeta, names)).toBe("—");
    });

    it("actorLabel resolves actor_id to display name or email, else raw id", () => {
        const users = [
            { id: "user-1", email: "admin@alcedo.dev", display_name: "Admin" },
            { id: "user-2", email: "customer@acme.example", display_name: null },
        ];
        expect(actorLabel(normalizeSystemLog(systemLogRow, "crm"), users)).toBe("Admin");
        expect(actorLabel(normalizeCollectionLog(collectionLogRow, "crm"), users)).toBe(
            "customer@acme.example",
        );
        const unknown = normalizeSystemLog(
            { ...systemLogRow, actor_id: "plugin-xyz" },
            "crm",
        );
        expect(actorLabel(unknown, users)).toBe("plugin-xyz");
        const noActor = normalizeSystemLog({ ...systemLogRow, actor_id: null }, "crm");
        expect(actorLabel(noActor, users)).toBe("—");
    });
});

describe("useVersionChangesStore", () => {
    it("fetchVersions and fetchAppVersions populate state", async () => {
        const store = useVersionChangesStore();
        await store.fetchVersions();
        await store.fetchAppVersions();
        expect(store.versions).toEqual(versions);
        expect(store.appVersions).toEqual(appVersions);
    });

    it("loadVersionChanges aggregates per-app logs and app_created edits", async () => {
        const store = useVersionChangesStore();
        await store.fetchVersions();
        await store.fetchAppVersions();
        await store.loadVersionChanges({ id: 2, version_name: "staging" });
        expect(store.error).toBeNull();
        // user-item (collection log) edits are never deployable and are not
        // listed as changes — only system-log edits + app_created
        expect(store.changes.map((c) => c.id)).toEqual([
            "system:12",
            "app_created:helpdesk",
        ]);
        // logs were requested with the picked version as X-App/X-Version
        // context, hiding already-deployed edits
        expect(mocks.listAppSystem).toHaveBeenCalledWith(
            { excludeDeployed: true, limit: 1000, offset: 0 },
            { app: "crm", version: "staging" },
        );
        expect(mocks.listCollections).not.toHaveBeenCalled();
    });

    it("loadAppChanges loads a single app from the URL context", async () => {
        const store = useVersionChangesStore();
        await store.fetchAppVersions();
        await store.loadAppChanges("helpdesk", "staging");
        expect(store.changes.map((c) => c.id)).toEqual([
            "app_created:helpdesk",
        ]);
        expect(mocks.listAppSystem).toHaveBeenCalledWith(
            { excludeDeployed: true, limit: 1000, offset: 0 },
            { app: "helpdesk", version: "staging" },
        );
    });

    it("selection helpers toggle, select all and clear", async () => {
        const store = useVersionChangesStore();
        await store.fetchVersions();
        await store.fetchAppVersions();
        await store.loadVersionChanges({ id: 2, version_name: "staging" });
        store.toggleSelect("system:12");
        expect(store.selectedIds).toEqual(["system:12"]);
        expect(store.selectedChanges.map((c) => c.id)).toEqual(["system:12"]);
        store.toggleSelect("system:12");
        expect(store.selectedIds).toEqual([]);
        store.toggleSelectAll();
        expect(store.allSelected).toBe(true);
        expect(store.selectedIds.length).toBe(2);
        store.toggleSelectAll();
        expect(store.selectedIds).toEqual([]);
        store.toggleSelectAll();
        store.clearSelection();
        expect(store.selectedIds).toEqual([]);
    });

    it("selectedEditIds is null without selection and includes app api_names with selection", async () => {
        const store = useVersionChangesStore();
        await store.fetchVersions();
        await store.fetchAppVersions();
        await store.loadVersionChanges({ id: 2, version_name: "staging" });
        expect(store.selectedEditIds).toBeNull();
        store.toggleSelectAll();
        // system-log edit id 12 + the app_created api_name — in selected mode
        // the core only deploys what is listed, so the api_name must be sent
        expect(store.selectedEditIds).toEqual([12, "helpdesk"]);
        store.toggleSelect("app_created:helpdesk");
        expect(store.selectedEditIds).toEqual([12]);
        store.toggleSelectAll();
        store.toggleSelect("system:12");
        // only the new app selected: its api_name is sent (edit_ids: [] would
        // select nothing in the core)
        expect(store.selectedEditIds).toEqual(["helpdesk"]);
    });

    it("deploy calls the SDK deploy method with the body and unwraps success incl. unsupported", async () => {
        mocks.versionsDeploy.mockResolvedValue({
            deployed: true,
            apps: { crm: { tables_written: 3 } },
            unsupported: [],
        });
        const store = useVersionChangesStore();
        const res = await store.deploy(2, buildDeployBody(null, false, "fail"));
        expect(res.ok).toBe(true);
        expect(res.deployed).toBe(true);
        expect(res.conflicts).toEqual([]);
        expect(res.unsupported).toEqual([]);
        expect(mocks.versionsDeploy).toHaveBeenCalledWith(
            2,
            {
                edit_ids: null,
                dry_run: false,
                on_conflict: "fail",
            },
            { timeout: 300_000 },
        );
    });

    it("deploy reads data.conflicts AND data.unsupported from the SDK AlcedoApiError (HTTP 409 fail)", async () => {
        const conflicts = [
            {
                type: "relation_missing_in_prod",
                app: "crm",
                detail: "collection 'invoices' does not exist in crm010production",
            },
        ];
        const unsupported = [
            {
                type: "field_type_change",
                app: "crm",
                collection: "contacts",
                detail: "field 'email' changed type from string to integer",
            },
        ];
        mocks.versionsDeploy.mockRejectedValue(
            new AlcedoApiError("conflicts found", undefined, "fail", 409, {
                deployed: false,
                conflicts,
                unsupported,
                message: "field changes a deploy cannot apply",
            }),
        );
        const store = useVersionChangesStore();
        const res = await store.deploy(2, buildDeployBody([12], false, "fail"));
        expect(res.ok).toBe(false);
        expect(res.httpStatus).toBe(409);
        expect(res.deployed).toBe(false);
        expect(res.conflicts).toEqual(conflicts);
        expect(res.unsupported).toEqual(unsupported);
        expect(res.message).toBe("conflicts found");
        expect(store.lastResult).toStrictEqual(res);
        // the check popup renders both lists
        expect(deployIssueReasons(res)).toEqual([
            "collection 'invoices' does not exist in crm010production",
            "field 'email' changed type from string to integer",
        ]);
    });

    it("deploy handles dry_run previews and non-API errors", async () => {
        mocks.versionsDeploy.mockResolvedValue({
            deployed: false,
            dry_run: true,
            edits: [{ id: 12 }, { id: 13 }],
            conflicts: [],
            unsupported: [
                { type: "field_deleted", app: "crm", detail: "field 'email' was deleted" },
            ],
        });
        const store = useVersionChangesStore();
        const preview = await store.deploy(2, buildDeployBody(null, true, "fail"));
        expect(preview.ok).toBe(true);
        expect(preview.dryRun).toBe(true);
        expect(preview.edits.length).toBe(2);
        // dry_run lists unsupported changes with ok:true (nothing written)
        expect(preview.unsupported).toHaveLength(1);
        expect(deployHasNoIssues(preview)).toBe(false);

        mocks.versionsDeploy.mockRejectedValue(new Error("network down"));
        const failed = await store.deploy(2, buildDeployBody(null, false, "fail"));
        expect(failed.ok).toBe(false);
        expect(failed.unsupported).toEqual([]);
        expect(failed.message).toBe("network down");
        expect(store.deploying).toBe(false);
    });
});
