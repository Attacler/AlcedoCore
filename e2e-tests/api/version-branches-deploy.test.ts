/**
 * Version-branches DEPLOY e2e test (HTTP API, no SDK) — companion to
 * version-branches.test.ts.
 *
 * Research-driven coverage of everything a version can deploy into
 * production, plus the refusal/error cases:
 *
 * Deployable objects (happy paths):
 *  - field add (plain + many_to_one relation) via wholesale deploy
 *  - field add via SELECTED deploy (edit_ids = [log row id])
 *  - collection metadata update (display_name)
 *  - menu, saved view, app setting created in the version
 *  - a NEW APP created directly on the version (app_created edit, edit_ids=[api_name])
 *  - a plugin install in the version (copied to the production app-version)
 *  - user-item edits: enumerated + conflict-checked, but never row-copied
 *  - collection deletion in a version writes no log row -> not a deployable
 *    edit (pinned); production keeps the collection
 *
 * Conflict / refusal cases:
 *  - dry_run never writes and never 409s, even with conflicts
 *  - on_conflict=fail -> 409 changed_on_both_sides; skip -> edit skipped;
 *    overwrite -> version wins
 *  - field type change / field deletion -> 409 unsupported, nothing written
 *  - auth: unauthenticated -> 401; platform version guards (deploy/revert
 *    production, delete production) -> 400/403; unknown ids -> 404;
 *    non-numeric id -> 400
 *  - create_version blank/duplicate name -> 400; create_app duplicate
 *    api_name -> 400 / unknown version -> 404; invalid collection names /
 *    duplicate fields / relation to missing collection -> 400
 *  - revert with nothing applied -> 200 no-op; double deploy -> no pending edits
 *  - revert undoes applied edits; re-deploy reapplies them
 *
 * Run (core must be running):
 *   cd e2e-tests
 *   CORE_URL=http://localhost npx tsx --test ./api/version-branches-deploy.test.ts
 *
 * Env overrides: CORE_URL, ADMIN_EMAIL, ADMIN_PASSWORD.
 */
import { describe, it, before, after } from "node:test";
import assert from "node:assert";

const API = process.env.CORE_URL ?? "http://localhost:8080";
const EMAIL = process.env.ADMIN_EMAIL ?? "admin@alcedo.dev";
const PASSWORD = process.env.ADMIN_PASSWORD ?? "admin123!";

const SUFFIX = Math.random().toString(36).slice(2, 8);
const APP = `e2edep_${SUFFIX}`;
const APP2 = `e2edep2_${SUFFIX}`;
const VERSION = `e2edep_${SUFFIX}`;
const VERSION2 = `e2edepb_${SUFFIX}`;
const NOTES = `dep_notes_${SUFFIX}`;
const NOTESX = `dep_notex_${SUFFIX}`; // for the field-type-change refusal
const NOTESY = `dep_notey_${SUFFIX}`; // for the field-deletion refusal
const NOTESZ = `dep_notez_${SUFFIX}`; // for the collection-deletion probe
const SETTING_KEY = `e2e_setting_${SUFFIX}`;

let cookie = "";
let appId = 0;
let app2Id = 0;
let versionId = 0;
let version2Id = 0;
let productionVersionId = 0;
let prodLinkId = 0; // app-version link id of (APP, production)
let versionLinkId = 0; // app-version link id of (APP, VERSION)
let itemId: unknown = null;

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

function headers(c?: string, app?: string, version?: string): Record<string, string> {
    const h: Record<string, string> = { "Content-Type": "application/json" };
    if (c) h["Cookie"] = c;
    if (app) h["X-App"] = app;
    if (version) h["X-Version"] = version;
    return h;
}

async function login(): Promise<string> {
    const res = await fetch(`${API}/api/platform/auth/login`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email: EMAIL, password: PASSWORD }),
        redirect: "manual",
    });
    const text = await res.text();
    if (res.status !== 200) throw new Error(`Login failed: ${res.status} ${text}`);
    const setCookie = res.headers.get("set-cookie") || "";
    const match = setCookie.match(/alcedo_session=([^;]+)/);
    if (!match) throw new Error(`No session cookie returned: ${setCookie}`);
    return `alcedo_session=${match[1]}`;
}

interface CallOptions {
    app?: string;
    version?: string;
    body?: unknown;
    cookieOverride?: string;
}

async function call(
    method: string,
    path: string,
    opts: CallOptions = {},
): Promise<{ status: number; json: any }> {
    const res = await fetch(`${API}${path}`, {
        method,
        headers: headers(opts.cookieOverride ?? cookie, opts.app, opts.version),
        body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
        redirect: "manual",
    });
    const text = await res.text();
    let json: any = null;
    try {
        json = JSON.parse(text);
    } catch {
        json = { raw: text };
    }
    return { status: res.status, json };
}

function data(res: { status: number; json: any }, what: string): any {
    assert.strictEqual(
        res.json?.status,
        "success",
        `${what}: expected JSend success, got ${JSON.stringify(res.json).slice(0, 300)}`,
    );
    return res.json.data;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function collectionList(app: string, version: string): Promise<any[]> {
    const res = await call("GET", "/api/app/collections", { app, version });
    return data(res, "list collections").collections;
}

async function collectionNames(app: string, version: string): Promise<string[]> {
    return (await collectionList(app, version)).map((c: any) => c.name ?? c.table ?? c.api_name);
}

async function getCollection(app: string, version: string, name: string): Promise<any> {
    const res = await call("GET", `/api/app/collections/${name}`, { app, version });
    return data(res, `get collection ${name}`);
}

async function itemCount(app: string, version: string, collection: string): Promise<number> {
    const res = await call("GET", `/api/app/items/${collection}?includeCount=true&limit=1`, {
        app,
        version,
    });
    const d = data(res, "list items");
    return d.total ?? d.count ?? (Array.isArray(d.data) ? d.data.length : 0);
}

/** Poll a dry-run deploy until `pred` matches the response data (activity logs are async). */
async function pollDryRun(
    vid: number,
    pred: (d: any) => boolean,
    timeoutMs = 25000,
    label = "condition",
): Promise<any> {
    const deadline = Date.now() + timeoutMs;
    let last: any = null;
    while (Date.now() < deadline) {
        const res = await call("POST", `/api/platform/versions/${vid}/deploy`, {
            body: { dry_run: true },
        });
        assert.strictEqual(res.status, 200, `dry-run: ${JSON.stringify(res.json).slice(0, 200)}`);
        last = data(res, "dry-run");
        if (pred(last)) return last;
        await sleep(500);
    }
    throw new Error(`pollDryRun timeout waiting for ${label}; last=${JSON.stringify(last).slice(0, 600)}`);
}

async function deploy(vid: number, body: unknown): Promise<{ status: number; json: any }> {
    return call("POST", `/api/platform/versions/${vid}/deploy`, { body });
}

async function appVersionLinks(): Promise<any[]> {
    return data(await call("GET", "/api/platform/app-versions"), "app-versions");
}

/** PUT the collection in `version` with `fields` = current fields + extras (keeps existing). */
async function updateCollectionFields(
    app: string,
    version: string,
    name: string,
    extras: any[],
    removed: string[] = [],
): Promise<void> {
    const current = await getCollection(app, version, name);
    const res = await call("PUT", `/api/app/collections/${name}`, {
        app,
        version,
        body: { fields: [...(current.fields ?? []), ...extras], removed_fields: removed },
    });
    data(res, `update collection ${name}`);
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

describe("version-branches deploy e2e", () => {
    before(async () => {
        cookie = await login();

        const app = await call("POST", "/api/platform/apps", {
            body: { name: `E2E dep ${SUFFIX}`, api_name: APP, version: "production" },
        });
        appId = data(app, "create app").id;

        const mk = async (name: string, fields: any[]) =>
            data(
                await call("POST", "/api/app/collections", {
                    app: APP,
                    version: "production",
                    body: { name, fields },
                }),
                `create ${name}`,
            );
        await mk(NOTES, [{ name: "title", type: "string", required: true }]);
        await mk(NOTESX, [
            { name: "title", type: "string" },
            { name: "extra", type: "string" },
        ]);
        await mk(NOTESY, [
            { name: "title", type: "string" },
            { name: "temp", type: "string" },
        ]);
        await mk(NOTESZ, [{ name: "title", type: "string" }]);
        // setting created in production BEFORE the version clone, so both
        // schemas carry the same row and edits conflict on the same target
        await call("POST", "/api/app/settings/batch", {
            app: APP,
            version: "production",
            body: { settings: { [SETTING_KEY]: "initial" } },
        });

        const item = await call("POST", `/api/app/items/${NOTES}`, {
            app: APP,
            version: "production",
            body: { title: "prod item" },
        });
        itemId = data(item, "create item").created[0].id;

        const versions = data(await call("GET", "/api/platform/versions"), "list versions");
        productionVersionId = versions.find((v: any) => v.version_name === "production").id;

        const ver = await call("POST", "/api/platform/versions", {
            body: { version_name: VERSION },
        });
        versionId = data(ver, "create version").id;

        const links = await appVersionLinks();
        prodLinkId = links.find((l: any) => l.api_name === APP && l.version_id === productionVersionId).id;
        versionLinkId = links.find((l: any) => l.api_name === APP && l.version_id === versionId).id;
        assert.ok(prodLinkId && versionLinkId, "app must be linked to production and the new version");
    });

    after(async () => {
        for (const vid of [versionId, version2Id]) {
            if (vid) await call("DELETE", `/api/platform/versions/${vid}`);
        }
        for (const aid of [appId, app2Id]) {
            if (aid) await call("DELETE", `/api/platform/apps/${aid}`);
        }
    });

    // -----------------------------------------------------------------------
    // Deployable object types
    // -----------------------------------------------------------------------

    it("deploys field adds (plain + many_to_one relation) wholesale", async () => {
        await updateCollectionFields(APP, VERSION, NOTES, [
            { name: "body", type: "string" },
            {
                name: "parent",
                type: "relationship",
                relationship_type: "many_to_one",
                related_collection: NOTES,
            },
        ]);

        const dry = await pollDryRun(
            versionId,
            (d) =>
                (d.edits ?? []).filter(
                    (e: any) => e.action === "item_created" && e.target === "alcedocore_fields",
                ).length >= 2,
            25000,
            "two field edits",
        );
        assert.ok(dry.edits.some((e: any) => e.action === "item_created" && e.target === "alcedocore_fields"));

        const res = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(res.status, 200, JSON.stringify(res.json).slice(0, 300));
        assert.strictEqual(data(res, "deploy").deployed, true);

        const prod = await getCollection(APP, "production", NOTES);
        const names = prod.fields.map((f: any) => f.name);
        assert.ok(names.includes("body"), `production ${NOTES} must have body: ${names}`);
        const parent = prod.fields.find((f: any) => f.name === "parent");
        assert.ok(parent, "production must have parent field");
        assert.strictEqual(parent.relationship_type, "many_to_one");
        assert.strictEqual(parent.related_collection, NOTES);
    });

    it("deploys a single field add via selected edit_ids", async () => {
        await updateCollectionFields(APP, VERSION, NOTES, [{ name: "tag", type: "string" }]);

        // wait until the previously applied field edits are marked applied and
        // only the new `tag` edit remains pending
        const dry = await pollDryRun(
            versionId,
            (d) => {
                const fe = (d.edits ?? []).filter(
                    (e: any) => e.action === "item_created" && e.target === "alcedocore_fields",
                );
                return fe.length === 1 && typeof fe[0].id === "number";
            },
            25000,
            "only the tag field edit pending",
        );
        const editId = dry.edits.find(
            (e: any) => e.action === "item_created" && e.target === "alcedocore_fields",
        ).id;

        const res = await deploy(versionId, { edit_ids: [editId], on_conflict: "overwrite" });
        assert.strictEqual(res.status, 200, JSON.stringify(res.json).slice(0, 300));
        const d = data(res, "selected deploy");
        assert.strictEqual(d.deployed, true);
        assert.ok(
            (d.apps ?? []).some((a: any) => a.app === APP && a.status === "selected"),
            `expected selected-mode result for ${APP}: ${JSON.stringify(d.apps)}`,
        );

        const prod = await getCollection(APP, "production", NOTES);
        assert.ok(
            prod.fields.map((f: any) => f.name).includes("tag"),
            "production must have tag field after selected deploy",
        );
    });

    it("deploys menus, saved views, collection metadata and app settings (wholesale copy)", async () => {
        // NOTE: only writes that go through the items service produce activity
        // log rows (deploy "edits"); the wholesale deploy additionally copies
        // the whole copy-set tables, so unlogged writes (display_name, saved
        // views) still land in production on a wholesale deploy.
        const cur = await getCollection(APP, VERSION, NOTES);
        let res = await call("PUT", `/api/app/collections/${NOTES}`, {
            app: APP,
            version: VERSION,
            body: { display_name: `Renamed ${SUFFIX}`, fields: cur.fields ?? [] },
        });
        data(res, "rename collection in version");

        res = await call("POST", "/api/app/menus", {
            app: APP,
            version: VERSION,
            body: { name: `e2e menu ${SUFFIX}` },
        });
        data(res, "create menu in version");

        res = await call("POST", `/api/app/collections/${NOTES}/views`, {
            app: APP,
            version: VERSION,
            body: { name: `e2e view ${SUFFIX}`, config: { sort: "title" } },
        });
        data(res, "create saved view in version");

        res = await call("POST", "/api/app/settings/batch", {
            app: APP,
            version: VERSION,
            body: { settings: { [SETTING_KEY]: "from-version" } },
        });
        data(res, "set app setting in version");

        // wait for the LOGGED edits (menu + setting); display_name/view are
        // copied wholesale without a log row
        await pollDryRun(
            versionId,
            (d) =>
                (d.edits ?? []).some(
                    (e: any) => e.action === "item_created" && e.target === "alcedocore_menus",
                ) && (d.edits ?? []).some((e: any) => e.action === "setting_changed"),
            25000,
            "menu + setting edits",
        );

        res = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(data(res, "deploy").deployed, true);

        const prod = await getCollection(APP, "production", NOTES);
        assert.strictEqual(prod.display_name, `Renamed ${SUFFIX}`);

        const menus = data(await call("GET", "/api/app/menus", { app: APP, version: "production" }), "menus");
        const menuList = Array.isArray(menus) ? menus : menus.menus ?? [];
        assert.ok(
            menuList.some((m: any) => m.name === `e2e menu ${SUFFIX}`),
            `production must have the menu: ${JSON.stringify(menuList)}`,
        );

        const views = data(
            await call("GET", `/api/app/collections/${NOTES}/views`, { app: APP, version: "production" }),
            "views",
        );
        const viewList = views.views ?? views;
        assert.ok(
            viewList.some((v: any) => v.name === `e2e view ${SUFFIX}`),
            `production must have the saved view: ${JSON.stringify(viewList)}`,
        );

        const settings = data(
            await call("GET", "/api/app/settings", { app: APP, version: "production" }),
            "settings",
        );
        assert.strictEqual(settings[SETTING_KEY], "from-version");
    });

    it("deploys a new app created directly on the version (app_created, edit_ids=[api_name])", async () => {
        const res = await call("POST", "/api/platform/apps", {
            body: { name: `E2E dep2 ${SUFFIX}`, api_name: APP2, version: VERSION },
        });
        app2Id = data(res, "create app on version").id;

        const dry = await pollDryRun(
            versionId,
            (d) => (d.edits ?? []).some((e: any) => e.source === "app" && e.action === "app_created"),
            25000,
            "app_created edit",
        );
        const appEdit = dry.edits.find((e: any) => e.action === "app_created");
        assert.strictEqual(appEdit.target, APP2);

        const dep = await deploy(versionId, { edit_ids: [APP2], on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        const d = data(dep, "app deploy");
        assert.strictEqual(d.deployed, true);
        assert.ok(
            (d.apps ?? []).some((a: any) => a.app === APP2 && a.status === "created"),
            `expected created app result: ${JSON.stringify(d.apps)}`,
        );

        const links = await appVersionLinks();
        assert.ok(
            links.some((l: any) => l.api_name === APP2 && l.version_id === productionVersionId),
            `${APP2} must be linked to production after deploy`,
        );
        // the production schema serves the new app
        const cols = await call("GET", "/api/app/collections", { app: APP2, version: "production" });
        assert.strictEqual(cols.json?.status, "success", JSON.stringify(cols.json).slice(0, 200));
    });

    it("copies a plugin install from the version to production on deploy", async () => {
        const res = await call("POST", "/api/platform/plugins/deploy", {
            body: {
                slug: "hello-world",
                plugin_version: "1.0.0",
                image: "localhost:5000/hello-world:1.0.0",
                registry_id: 1,
                app_version_id: versionLinkId,
                scope: "app",
                env: {},
                granted_scopes: ["kv.all"],
            },
        });
        assert.strictEqual(res.status, 200, `plugin deploy: ${JSON.stringify(res.json).slice(0, 300)}`);

        await pollDryRun(
            versionId,
            (d) => (d.edits ?? []).some((e: any) => e.action === "plugin_deployed"),
            25000,
            "plugin_deployed edit",
        );

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(data(dep, "deploy with plugin").deployed, true);

        const plugins = data(await call("GET", "/api/platform/plugins"), "plugins").plugins;
        const hw = plugins.find((p: any) => p.slug === "hello-world");
        assert.ok(hw, "hello-world must be in the catalog");
        assert.ok(
            (hw.installations ?? []).some((i: any) => i.app_version_id === prodLinkId),
            `hello-world must be installed on (${APP}, production) link ${prodLinkId}: ${JSON.stringify(hw.installations)}`,
        );
    });

    it("enumerates user-item edits but never copies item rows", async () => {
        const res = await call("POST", `/api/app/items/${NOTES}`, {
            app: APP,
            version: VERSION,
            body: { title: "version-only item" },
        });
        data(res, "create item in version");

        const dry = await pollDryRun(
            versionId,
            (d) =>
                (d.edits ?? []).some(
                    (e: any) => e.source === "collection" && e.action === "item_created" && e.collection === NOTES,
                ),
            25000,
            "collection-log item_created edit",
        );
        const edit = dry.edits.find((e: any) => e.source === "collection" && e.action === "item_created");

        const dep = await deploy(versionId, { edit_ids: [edit.id], on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        const d = data(dep, "deploy item edit");
        assert.ok(
            (d.skipped ?? []).some(
                (s: any) => s.reason === "user items are excluded from the copy set",
            ),
            `item edit must be skipped: ${JSON.stringify(d.skipped)}`,
        );

        // production still has exactly the original item
        assert.strictEqual(await itemCount(APP, "production", NOTES), 1);
    });

    it("collection deletion in a version produces no deploy edit; production keeps it", async () => {
        // delete_collection uses raw SQL, so no activity-log row is written and
        // the delete is not a deployable edit — pin that behavior.
        const res = await call("DELETE", `/api/app/collections/${NOTESZ}`, {
            app: APP,
            version: VERSION,
        });
        data(res, "delete collection in version");

        // give the async log channel a chance, then assert no collection_deleted
        await sleep(3000);
        const dry = data(await deploy(versionId, { dry_run: true }), "dry-run");
        assert.ok(
            !(dry.edits ?? []).some((e: any) => e.action === "collection_deleted"),
            `collection_deleted must not appear: ${JSON.stringify(dry.edits)}`,
        );

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(data(dep, "deploy after delete").deployed, true);
        const prodCols = await collectionNames(APP, "production");
        assert.ok(prodCols.includes(NOTESZ), `production must still list ${NOTESZ}`);
    });

    // -----------------------------------------------------------------------
    // Conflicts
    // -----------------------------------------------------------------------

    it("changed_on_both_sides: dry-run reports it, fail -> 409, skip -> skipped, overwrite -> version wins", async () => {
        // change the same app setting on both sides after the clone (the
        // setting row exists in both schemas with the same key)
        let res = await call("PUT", `/api/app/settings/${SETTING_KEY}`, {
            app: APP,
            version: "production",
            body: { value: "prod-side" },
        });
        data(res, "prod-side setting");
        res = await call("PUT", `/api/app/settings/${SETTING_KEY}`, {
            app: APP,
            version: VERSION,
            body: { value: "ver-side" },
        });
        data(res, "version-side setting");

        const dry = await pollDryRun(
            versionId,
            (d) =>
                (d.conflicts ?? []).some(
                    (c: any) => c.type === "changed_on_both_sides" && c.app === APP,
                ) && (d.edits ?? []).some((e: any) => e.action === "setting_changed"),
            25000,
            "changed_on_both_sides conflict",
        );
        // dry-run must NOT 409 even with conflicts present
        assert.strictEqual(dry.deployed, false);
        assert.strictEqual(dry.conflicts.find((c: any) => c.type === "changed_on_both_sides").app, APP);

        const prodSetting = async () =>
            data(await call("GET", "/api/app/settings", { app: APP, version: "production" }), "settings")[
                SETTING_KEY
            ];

        // fail -> 409, nothing written
        let dep = await deploy(versionId, { on_conflict: "fail" });
        assert.strictEqual(dep.status, 409, `expected 409, got ${dep.status}`);
        assert.strictEqual(dep.json?.status, "fail");
        assert.strictEqual(dep.json.data?.deployed, false);
        assert.ok(
            (dep.json.data?.conflicts ?? []).some((c: any) => c.type === "changed_on_both_sides"),
        );
        assert.strictEqual(await prodSetting(), "prod-side");

        // skip -> conflicting edit skipped, rest applied
        dep = await deploy(versionId, { on_conflict: "skip" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        const skipData = data(dep, "skip deploy");
        assert.strictEqual(skipData.deployed, true);
        assert.ok(
            (skipData.skipped ?? []).some((s: any) => s.reason === "conflict with production"),
            `expected conflict skip reason: ${JSON.stringify(skipData.skipped)}`,
        );
        assert.strictEqual(await prodSetting(), "prod-side", "skip must not overwrite the conflicted edit");

        // overwrite -> version wins (fresh version-side edit, still conflicting)
        res = await call("PUT", `/api/app/settings/${SETTING_KEY}`, {
            app: APP,
            version: VERSION,
            body: { value: "ver-wins" },
        });
        data(res, "version-side setting 2");
        await pollDryRun(
            versionId,
            (d) =>
                (d.conflicts ?? []).some((c: any) => c.type === "changed_on_both_sides") &&
                // BOTH unapplied version-side setting edits (ver-side + ver-wins)
                // must be visible, otherwise the deploy below can run before the
                // async log row lands and leave the newest edit pending forever
                (d.edits ?? []).filter((e: any) => e.action === "setting_changed").length >= 2,
            25000,
            "second conflict",
        );
        dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        assert.strictEqual(data(dep, "overwrite deploy").deployed, true);
        assert.strictEqual(await prodSetting(), "ver-wins", "overwrite must let the version win");
    });

    // -----------------------------------------------------------------------
    // Revert / redeploy / idempotency
    // -----------------------------------------------------------------------

    it("double deploy has no pending edits; revert undoes; redeploy reapplies", async () => {
        // everything applied so far -> no pending edits
        const dry = await pollDryRun(versionId, (d) => (d.edits ?? []).length === 0, 30000, "no pending edits");
        assert.strictEqual(dry.edits.length, 0);

        const beforeCols = (await collectionNames(APP, "production")).sort();
        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(data(dep, "second deploy").deployed, true);
        assert.deepStrictEqual((await collectionNames(APP, "production")).sort(), beforeCols, "no duplicates");

        // revert: undoes applied edits; the new app is skipped, not dropped
        const rev = await call("POST", `/api/platform/versions/${versionId}/revert`);
        assert.strictEqual(rev.status, 200, JSON.stringify(rev.json).slice(0, 300));
        const revData = data(rev, "revert");
        assert.ok((revData.reverted ?? []).length >= 1, "revert must revert something");
        assert.ok(
            (revData.skipped ?? []).some(
                (s: any) => s.reason === "new apps are not reverted automatically",
            ),
            `new app must be skipped on revert: ${JSON.stringify(revData.skipped)}`,
        );
        // the field adds on NOTES are reverted out of the production metadata
        const prodNotes = await getCollection(APP, "production", NOTES);
        assert.ok(
            !prodNotes.fields.map((f: any) => f.name).includes("tag"),
            "tag field must be reverted out of production metadata",
        );

        // redeploy: edits are pending again and apply cleanly
        await pollDryRun(
            versionId,
            (d) => (d.edits ?? []).length >= 1,
            25000,
            "edits pending again after revert",
        );
        const dep2 = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(data(dep2, "redeploy").deployed, true);
        const prodNotes2 = await getCollection(APP, "production", NOTES);
        assert.ok(prodNotes2.fields.map((f: any) => f.name).includes("tag"), "tag must be back after redeploy");
    });

    // -----------------------------------------------------------------------
    // Refusals: unsupported field changes (LAST: they stay pending and would
    // make every later real deploy refuse with 409)
    // -----------------------------------------------------------------------

    it("refuses field type change (409 unsupported, nothing written)", async () => {
        // NOTE: a type change via the collections PUT updates the alcedocore_fields
        // row with raw SQL — no activity-log row, so the deploy cannot see it.
        // Drive a deploy-visible edit through the items API instead: PATCH the
        // field row's options in the version -> item_updated edit on
        // alcedocore_fields -> the deploy classifies it as field_type_change
        // (version row signature differs from production's).
        const list = await call(
            "GET",
            "/api/app/items/alcedocore_fields?filter%5Bapi_name%5D%5B_eq%5D=extra&limit=10",
            { app: APP, version: VERSION },
        );
        const rows = data(list, "list field rows").data ?? [];
        const extra = rows.find((r: any) => r.api_name === "extra");
        assert.ok(extra, `extra field row must exist in the version: ${JSON.stringify(rows)}`);
        // PATCH on the collection path (ItemsService -> logged); the
        // /{collection}/{id} PATCH goes through update_recursive, which only
        // accepts registered collections.
        const patch = await call(
            "PATCH",
            "/api/app/items/alcedocore_fields?filter%5Bapi_name%5D%5B_eq%5D=extra",
            {
                app: APP,
                version: VERSION,
                body: { options: { ...(extra.options ?? {}), type: "int" } }, // was string
            },
        );
        data(patch, "patch field type in version");

        const dry = await pollDryRun(
            versionId,
            (d) => (d.unsupported ?? []).some((u: any) => u.type === "field_type_change"),
            25000,
            "field_type_change unsupported",
        );
        assert.ok(dry.unsupported.some((u: any) => u.type === "field_type_change"));

        const dep = await deploy(versionId, { on_conflict: "fail" });
        assert.strictEqual(dep.status, 409, `expected 409, got ${dep.status}`);
        assert.strictEqual(dep.json?.status, "fail");
        assert.strictEqual(dep.json.data?.deployed, false);
        assert.ok(
            (dep.json.data?.unsupported ?? []).some((u: any) => u.type === "field_type_change"),
            JSON.stringify(dep.json.data?.unsupported),
        );
        assert.ok(dep.json.data?.message, "refusal message expected");

        const prod = await getCollection(APP, "production", NOTESX);
        const prodExtra = prod.fields.find((f: any) => f.name === "extra");
        assert.strictEqual(prodExtra?.type, "string", "production field type must be unchanged");
    });

    it("refuses field deletion on an existing collection (409 unsupported, even with overwrite)", async () => {
        // Remove `temp` via the collections PUT: the field row is deleted through
        // the items service, which writes the item_deleted activity-log row the
        // deploy reads. (The field must NOT also appear in `fields` — omitted
        // fields are kept unless listed in removed_fields.)
        const cur = await getCollection(APP, VERSION, NOTESY);
        const keep = (cur.fields ?? []).filter((f: any) => f.name !== "temp");
        const res = await call("PUT", `/api/app/collections/${NOTESY}`, {
            app: APP,
            version: VERSION,
            body: { fields: keep, removed_fields: ["temp"] },
        });
        data(res, "remove temp field in version");

        await pollDryRun(
            versionId,
            (d) => (d.unsupported ?? []).some((u: any) => u.type === "field_deleted"),
            25000,
            "field_deleted unsupported",
        );

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        // unsupported refusals apply regardless of on_conflict
        assert.strictEqual(dep.status, 409, `expected 409, got ${dep.status}`);
        assert.ok(
            (dep.json.data?.unsupported ?? []).some((u: any) => u.type === "field_deleted"),
            JSON.stringify(dep.json.data?.unsupported),
        );

        const prod = await getCollection(APP, "production", NOTESY);
        assert.ok(
            prod.fields.map((f: any) => f.name).includes("temp"),
            "production must still have the temp field",
        );
    });

    // -----------------------------------------------------------------------
    // Error / refusal cases on the version & app API
    // -----------------------------------------------------------------------

    it("rejects unauthenticated platform calls with 401", async () => {
        // NOTE: bodies must be *valid* JSON payloads — an empty/invalid body is
        // rejected with 422 by the JSON extractor before the auth extractor runs.
        const cases: [string, string, unknown][] = [
            ["GET", "/api/platform/versions", undefined],
            ["POST", "/api/platform/versions", { version_name: `unauth_${SUFFIX}` }],
            ["DELETE", `/api/platform/versions/${versionId}`, undefined],
            ["POST", `/api/platform/versions/${versionId}/deploy`, { dry_run: true }],
            ["POST", `/api/platform/versions/${versionId}/revert`, {}],
        ];
        for (const [method, path, body] of cases) {
            const res = await call(method, path, { cookieOverride: "", body });
            assert.strictEqual(res.status, 401, `${method} ${path} must be 401, got ${res.status}`);
            assert.strictEqual(res.json?.status, "error");
        }
    });

    it("validates version creation and guards the production version", async () => {
        let res = await call("POST", "/api/platform/versions", { body: { version_name: "   " } });
        assert.strictEqual(res.status, 400, JSON.stringify(res.json));
        assert.match(res.json?.message ?? "", /version_name is required/);

        res = await call("POST", "/api/platform/versions", { body: { version_name: VERSION } });
        assert.strictEqual(res.status, 400, `duplicate: ${JSON.stringify(res.json).slice(0, 200)}`);
        assert.match(res.json?.message ?? "", /already exists/);

        res = await deploy(productionVersionId, {});
        assert.strictEqual(res.status, 400, `deploy production: ${JSON.stringify(res.json).slice(0, 200)}`);
        assert.match(res.json?.message ?? "", /cannot be deployed onto itself/);

        res = await call("POST", `/api/platform/versions/${productionVersionId}/revert`);
        assert.strictEqual(res.status, 400);
        assert.match(res.json?.message ?? "", /cannot be reverted/);

        res = await call("DELETE", `/api/platform/versions/${productionVersionId}`);
        assert.strictEqual(res.status, 403);
        assert.match(res.json?.message ?? "", /production version cannot be deleted/);
    });

    it("404s on unknown version ids and 400s on non-numeric ids", async () => {
        let res = await deploy(999999, {});
        assert.strictEqual(res.status, 404, JSON.stringify(res.json).slice(0, 200));

        res = await call("POST", "/api/platform/versions/999999/revert");
        assert.strictEqual(res.status, 404);

        res = await call("DELETE", "/api/platform/versions/999999");
        assert.strictEqual(res.status, 404);

        res = await deploy("abc" as any, {});
        assert.ok(res.status === 400 || res.status === 422, `non-numeric id: got ${res.status}`);
    });

    it("validates app creation", async () => {
        let res = await call("POST", "/api/platform/apps", {
            body: { name: "", api_name: `x_${SUFFIX}`, version: "production" },
        });
        assert.strictEqual(res.status, 400);
        assert.match(res.json?.message ?? "", /name is required/);

        res = await call("POST", "/api/platform/apps", {
            body: { name: "dup", api_name: APP, version: "production" },
        });
        assert.strictEqual(res.status, 400);
        assert.match(res.json?.message ?? "", /already exists/);

        res = await call("POST", "/api/platform/apps", {
            body: { name: "noversion", api_name: `x_${SUFFIX}`, version: "no_such_version" },
        });
        assert.strictEqual(res.status, 404);
        assert.match(res.json?.message ?? "", /Version not found/);
    });

    it("validates collection creation inside a version", async () => {
        let res = await call("POST", "/api/app/collections", {
            app: APP,
            version: VERSION,
            body: { name: "alcedocore_evil", fields: [{ name: "a", type: "string" }] },
        });
        assert.strictEqual(res.status, 400, `reserved prefix: ${JSON.stringify(res.json).slice(0, 200)}`);

        res = await call("POST", "/api/app/collections", {
            app: APP,
            version: VERSION,
            body: {
                name: `dup_${SUFFIX}`,
                fields: [
                    { name: "a", type: "string" },
                    { name: "a", type: "string" },
                ],
            },
        });
        assert.strictEqual(res.status, 400, `duplicate fields: ${JSON.stringify(res.json).slice(0, 200)}`);

        res = await call("POST", "/api/app/collections", {
            app: APP,
            version: VERSION,
            body: {
                name: `rel_${SUFFIX}`,
                fields: [
                    {
                        name: "r",
                        type: "relationship",
                        relationship_type: "many_to_one",
                        related_collection: "no_such_collection",
                    },
                ],
            },
        });
        assert.strictEqual(res.status, 400, `missing related collection: ${JSON.stringify(res.json).slice(0, 250)}`);
    });

    it("revert with nothing applied is a 200 no-op", async () => {
        const ver = await call("POST", "/api/platform/versions", { body: { version_name: VERSION2 } });
        version2Id = data(ver, "create second version").id;

        const rev = await call("POST", `/api/platform/versions/${version2Id}/revert`);
        assert.strictEqual(rev.status, 200, JSON.stringify(rev.json).slice(0, 300));
        const d = data(rev, "no-op revert");
        assert.deepStrictEqual(d.reverted ?? [], []);

        // and a second revert is still a no-op
        const rev2 = await call("POST", `/api/platform/versions/${version2Id}/revert`);
        assert.strictEqual(data(rev2, "second revert").reverted.length, 0);
    });
});
