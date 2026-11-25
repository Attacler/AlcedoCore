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
 *  - a plugin install in the version (copied to the production app-version);
 *    re-deploys dedup the install; route mappings on the version's install are
 *    remapped to the production install id
 *  - a NEW COLLECTION created in the version lands in production (structure +
 *    metadata); dry-run flags it as relation_missing_in_prod, fail -> 409
 *  - roles created in the version are NEVER deployed (excluded copy set):
 *    selected mode skips the edit ("target is not in the copy set"),
 *    wholesale leaves production's roles untouched
 *  - user-item edits: enumerated + conflict-checked, but never row-copied
 *    (same item changed on both sides -> changed_on_both_sides on the
 *    collection log; production row keeps its own value)
 *  - collection deletion in a version writes no log row -> not a deployable
 *    edit (pinned); production keeps the collection
 *  - edit_ids: [] or non-matching entries select nothing and write nothing;
 *    mixed edit_ids (log-row id + app api_name in one call) and dry_run +
 *    edit_ids combined
 *  - wholesale deploy does NOT clobber an app with no pending edits
 *    (production-side change on an untouched app survives)
 *  - cross-app relation deploy: two apps with a cross-app FK deploy in one
 *    wholesale run, dependency (related app) first; the prod FK is usable
 *  - plugin version change on re-deploy re-provisions the production install
 *    (runtime_provisioned: true), still deduplicated to one install
 *  - revert applies user-item inverses to production rows (pinned quirk:
 *    item_created inverse DELETES the prod row with that item id even though
 *    the row was never copied); every deploy/revert response carries the
 *    no-backup warning
 *
 * Conflict / refusal cases:
 *  - dry_run never writes and never 409s, even with conflicts
 *  - on_conflict=fail -> 409 changed_on_both_sides; skip -> edit skipped;
 *    overwrite -> version wins
 *  - field type change / field deletion / relation-target change -> 409
 *    unsupported, nothing written
 *  - auth: unauthenticated -> 401; non-admin user -> 403; platform version
 *    guards (deploy/revert production, delete production) -> 400/403;
 *    unknown ids -> 404; non-numeric id -> 400
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
const NEWCOL = `dep_newcol_${SUFFIX}`; // created in the version, deployed to production
const ROLE_NAME = `e2e_role_${SUFFIX}`; // must never land in production
const SETTING_KEY = `e2e_setting_${SUFFIX}`;
const APP4 = `e2edep4_${SUFFIX}`; // created on the version for the mixed edit_ids test
// apps created on production BEFORE the version clone (so the version inherits them)
const UNTOUCHED_APP = `e2edepu_${SUFFIX}`; // edited only in production -> must survive deploys
const UCOL = `dep_ucol_${SUFFIX}`;
const XAPP = `e2edeqx_${SUFFIX}`; // cross-app pair: XAPP.yref -> YAPP
const XCOL = `dep_xcol_${SUFFIX}`;
const YAPP = `e2edeqy_${SUFFIX}`;
const YCOL = `dep_ycol_${SUFFIX}`;
const VERSION3 = `e2edepc_${SUFFIX}`; // isolated version for the revert item-inverse pin

let cookie = "";
let appId = 0;
let app2Id = 0;
let app4Id = 0;
let untouchedAppId = 0;
let xAppId = 0;
let yAppId = 0;
let versionId = 0;
let version2Id = 0;
let version3Id = 0;
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

async function login(email = EMAIL, password = PASSWORD): Promise<string> {
    const res = await fetch(`${API}/api/platform/auth/login`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, password }),
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
        // extra apps on production, cloned into the version below
        const mkApp = async (api_name: string) =>
            data(
                await call("POST", "/api/platform/apps", {
                    body: { name: `E2E dep ${api_name}`, api_name, version: "production" },
                }),
                `create app ${api_name}`,
            ).id;
        const mkIn = async (app: string, name: string) =>
            data(
                await call("POST", "/api/app/collections", {
                    app,
                    version: "production",
                    body: { name, fields: [{ name: "title", type: "string" }] },
                }),
                `create ${name} in ${app}`,
            );
        untouchedAppId = await mkApp(UNTOUCHED_APP);
        await mkIn(UNTOUCHED_APP, UCOL);
        xAppId = await mkApp(XAPP);
        await mkIn(XAPP, XCOL);
        yAppId = await mkApp(YAPP);
        await mkIn(YAPP, YCOL);
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
        for (const vid of [versionId, version2Id, version3Id]) {
            if (vid) await call("DELETE", `/api/platform/versions/${vid}`);
        }
        for (const aid of [appId, app2Id, app4Id, untouchedAppId, xAppId, yAppId]) {
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

    it("mixed edit_ids (log-row id + app api_name in one call) and dry_run + edit_ids combined", async () => {
        // a new app on the version (app_created edit, selected by api_name)
        const res = await call("POST", "/api/platform/apps", {
            body: { name: `E2E dep4 ${SUFFIX}`, api_name: APP4, version: VERSION },
        });
        app4Id = data(res, "create app4 on version").id;
        // plus one pending field edit on APP (selected by numeric log-row id)
        await updateCollectionFields(APP, VERSION, NOTES, [{ name: "mixed_probe", type: "string" }]);

        const dry = await pollDryRun(
            versionId,
            (d) => {
                const appEdits = (d.edits ?? []).filter(
                    (e: any) => e.source === "app" && e.action === "app_created" && e.target === APP4,
                );
                const fieldEdits = (d.edits ?? []).filter(
                    (e: any) => e.app === APP && e.action === "item_created" && e.target === "alcedocore_fields",
                );
                return appEdits.length === 1 && fieldEdits.length === 1 && typeof fieldEdits[0].id === "number";
            },
            25000,
            "app_created + one field edit pending",
        );
        const fieldEditId = dry.edits.find(
            (e: any) => e.app === APP && e.target === "alcedocore_fields",
        ).id;

        // dry_run + edit_ids: reports only the selected edit, writes nothing, never 409s
        const selDry = await deploy(versionId, { dry_run: true, edit_ids: [fieldEditId] });
        assert.strictEqual(selDry.status, 200, JSON.stringify(selDry.json).slice(0, 300));
        const selDryData = data(selDry, "selected dry-run");
        assert.strictEqual(selDryData.deployed, false);
        assert.strictEqual(selDryData.edits.length, 1);
        assert.strictEqual(selDryData.edits[0].id, fieldEditId);
        assert.ok(
            !(await getCollection(APP, "production", NOTES)).fields.some((f: any) => f.name === "mixed_probe"),
            "dry-run must not write",
        );

        // mixed edit_ids: numeric log-row id + app api_name in ONE call
        const dep = await deploy(versionId, { edit_ids: [fieldEditId, APP4], on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        const d = data(dep, "mixed edit_ids deploy");
        assert.strictEqual(d.deployed, true);
        assert.ok(
            (d.apps ?? []).some((a: any) => a.app === APP && a.status === "selected"),
            `expected selected result for ${APP}: ${JSON.stringify(d.apps)}`,
        );
        assert.ok(
            (d.apps ?? []).some((a: any) => a.app === APP4 && a.status === "created"),
            `expected created result for ${APP4}: ${JSON.stringify(d.apps)}`,
        );
        assert.ok(
            (await getCollection(APP, "production", NOTES)).fields.some((f: any) => f.name === "mixed_probe"),
            "selected field edit must land in production",
        );
        assert.ok(
            (await appVersionLinks()).some((l: any) => l.api_name === APP4 && l.version_id === productionVersionId),
            `${APP4} must be linked to production after the mixed deploy`,
        );
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

    it("deploys a new collection created in the version (dry-run flags relation_missing_in_prod, fail -> 409)", async () => {
        const res = await call("POST", "/api/app/collections", {
            app: APP,
            version: VERSION,
            body: { name: NEWCOL, fields: [{ name: "label", type: "string" }] },
        });
        data(res, "create new collection in version");

        const dry = await pollDryRun(
            versionId,
            (d) => (d.edits ?? []).some((e: any) => e.action === "collection_created"),
            25000,
            "collection_created edit",
        );
        assert.ok(
            (dry.conflicts ?? []).some(
                (c: any) => c.type === "relation_missing_in_prod" && c.collection === NEWCOL,
            ),
            `dry-run must flag the new collection as missing in production: ${JSON.stringify(dry.conflicts)}`,
        );

        // fail mode refuses on that conflict, nothing written
        const fail = await deploy(versionId, { on_conflict: "fail" });
        assert.strictEqual(fail.status, 409, `expected 409, got ${fail.status}`);
        assert.ok(
            (fail.json.data?.conflicts ?? []).some((c: any) => c.type === "relation_missing_in_prod"),
        );
        assert.ok(!(await collectionNames(APP, "production")).includes(NEWCOL), "fail must not write");

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        assert.strictEqual(data(dep, "deploy new collection").deployed, true);

        assert.ok(
            (await collectionNames(APP, "production")).includes(NEWCOL),
            "production must list the new collection after deploy",
        );
        // the physical table exists too: an item can be written in production
        const item = await call("POST", `/api/app/items/${NEWCOL}`, {
            app: APP,
            version: "production",
            body: { label: "prod row" },
        });
        data(item, "create item in production new collection");
        assert.strictEqual(await itemCount(APP, "production", NEWCOL), 1);
    });

    it("never deploys roles (excluded copy set): selected edit skipped, wholesale leaves production roles untouched", async () => {
        const prodRolesBefore =
            data(await call("GET", "/api/app/roles", { app: APP, version: "production" }), "prod roles").data ??
            [];
        const prodNames = prodRolesBefore.map((r: any) => r.name).sort();

        const res = await call("POST", "/api/app/roles", {
            app: APP,
            version: VERSION,
            body: { name: ROLE_NAME, description: "must not deploy" },
        });
        data(res, "create role in version");

        const dry = await pollDryRun(
            versionId,
            (d) =>
                (d.edits ?? []).some((e: any) => e.action === "role_created" && e.target === "alcedocore_roles"),
            25000,
            "role_created edit",
        );
        const roleEdit = dry.edits.find((e: any) => e.action === "role_created");
        assert.ok(typeof roleEdit.id === "number", "role edit must carry a log-row id");

        // selected mode: target is outside the copy set -> skipped, nothing written
        const sel = await deploy(versionId, { edit_ids: [roleEdit.id], on_conflict: "overwrite" });
        assert.strictEqual(sel.status, 200, JSON.stringify(sel.json).slice(0, 300));
        const selData = data(sel, "selected role deploy");
        assert.strictEqual(selData.deployed, true);
        assert.ok(
            (selData.skipped ?? []).some((s: any) => s.reason === "target is not in the copy set"),
            `role edit must be skipped as outside the copy set: ${JSON.stringify(selData.skipped)}`,
        );

        // wholesale: the roles table is not in the copy set, production keeps its own rows
        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(data(dep, "wholesale after role").deployed, true);
        const prodRolesAfter =
            data(await call("GET", "/api/app/roles", { app: APP, version: "production" }), "prod roles after")
                .data ?? [];
        assert.ok(
            !prodRolesAfter.some((r: any) => r.name === ROLE_NAME),
            `role ${ROLE_NAME} must not land in production: ${JSON.stringify(prodRolesAfter)}`,
        );
        assert.deepStrictEqual(prodRolesAfter.map((r: any) => r.name).sort(), prodNames);
    });

    it("flags a collection-log conflict when the same item changed on both sides; fail -> 409; production row never overwritten", async () => {
        // version schemas are cloned structure-only, so the same item id must
        // be created explicitly on both sides (the create API accepts an `id`)
        let res = await call("POST", `/api/app/items/${NOTES}`, {
            app: APP,
            version: VERSION,
            body: { title: "ver-item" },
        });
        const id = String(data(res, "create item in version").created[0].id);
        res = await call("POST", `/api/app/items/${NOTES}`, {
            app: APP,
            version: "production",
            body: { id, title: "prod-item" },
        });
        data(res, "create same-id item in production");
        res = await call("PATCH", `/api/app/items/${NOTES}/${id}`, {
            app: APP,
            version: VERSION,
            body: { title: "ver-updated" },
        });
        data(res, "update item in version");

        const dry = await pollDryRun(
            versionId,
            (d) =>
                (d.conflicts ?? []).some(
                    (c: any) => c.type === "changed_on_both_sides" && c.collection === NOTES,
                ),
            25000,
            "item-level changed_on_both_sides conflict",
        );
        assert.ok(
            dry.conflicts.find((c: any) => c.type === "changed_on_both_sides" && c.collection === NOTES),
        );

        const fail = await deploy(versionId, { on_conflict: "fail" });
        assert.strictEqual(fail.status, 409, `expected 409, got ${fail.status}`);
        assert.ok(
            (fail.json.data?.conflicts ?? []).some(
                (c: any) => c.type === "changed_on_both_sides" && c.collection === NOTES,
            ),
        );

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        assert.strictEqual(data(dep, "overwrite with item conflict").deployed, true);

        // user item rows are never copied: production keeps its own value
        const prodItem = data(
            await call("GET", `/api/app/items/${NOTES}/${id}`, { app: APP, version: "production" }),
            "prod item",
        );
        assert.strictEqual(prodItem.title, "prod-item", "production item must keep its own value");
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

    it("dedups plugin installs across re-deploys and remaps route mappings to the production install", async () => {
        // several wholesale deploys have run by now (tests above); the
        // production install of hello-world must still be exactly one row
        const plugins = data(await call("GET", "/api/platform/plugins"), "plugins").plugins;
        const hw = plugins.find((p: any) => p.slug === "hello-world");
        assert.ok(hw, "hello-world must be in the catalog");
        const installs = hw.installations ?? [];
        const prodInstalls = installs.filter((i: any) => i.app_version_id === prodLinkId);
        assert.strictEqual(
            prodInstalls.length,
            1,
            `exactly one production install after several deploys: ${JSON.stringify(installs)}`,
        );
        const verInstalls = installs.filter((i: any) => i.app_version_id === versionLinkId);
        assert.strictEqual(verInstalls.length, 1, "the version keeps its own install");
        const prodInstallId = prodInstalls[0].id;
        const verInstallId = verInstalls[0].id;

        // a route mapping pointing at the VERSION's install (route mappings are
        // global, so create it without an X-Version header = production context)
        const prefix = `/e2e-rm-${SUFFIX}`;
        const created = await call("POST", "/api/platform/settings/route-mappings", {
            body: { domain: null, path_prefix: prefix, install_id: verInstallId, is_active: true },
        });
        const mapping = data(created, "create route mapping");
        assert.strictEqual(mapping.install_id, verInstallId);

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(data(dep, "deploy for route mapping").deployed, true);

        const list = data(await call("GET", "/api/platform/settings/route-mappings"), "route mappings");
        const after = (list.mappings ?? []).find((m: any) => m.path_prefix === prefix);
        assert.ok(after, "mapping must still exist after deploy");
        assert.strictEqual(
            after.install_id,
            prodInstallId,
            `mapping must be remapped to the production install: ${JSON.stringify(after)}`,
        );

        await call("DELETE", `/api/platform/settings/route-mappings/${mapping.id}`);
    });

    it("empty or non-matching edit_ids select nothing and write nothing", async () => {
        const pendingBefore = (data(await deploy(versionId, { dry_run: true }), "dry-run 0").edits ?? []).length;

        let dep = await deploy(versionId, { edit_ids: [] });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        let d = data(dep, "empty edit_ids");
        assert.strictEqual(d.deployed, true);
        assert.deepStrictEqual(d.apps ?? [], [], "no app may be written for an empty selection");

        dep = await deploy(versionId, { edit_ids: ["no_such_app_xyz", 424242] });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        d = data(dep, "non-matching edit_ids");
        assert.strictEqual(d.deployed, true);
        assert.deepStrictEqual(d.apps ?? [], [], "non-matching entries must be ignored");

        const dry = data(await deploy(versionId, { dry_run: true }), "dry-run");
        assert.strictEqual(
            (dry.edits ?? []).length,
            pendingBefore,
            `nothing may become pending: before=${pendingBefore} after=${(dry.edits ?? []).length} edits=${JSON.stringify(
                (dry.edits ?? []).map((e: any) => [e.app, e.source, e.action, e.target, e.collection, e.id]),
            )}`,
        );
        assert.match(dry.warning ?? "", /No data backup/, "dry-run response must carry the no-backup warning");
    });

    it("wholesale deploy does not clobber an app with no pending edits (prod-side change survives)", async () => {
        // production-side-only edit on the untouched app (after the clone)
        const cur = await getCollection(UNTOUCHED_APP, "production", UCOL);
        const renamed = `Prod-only ${SUFFIX}`;
        let res = await call("PUT", `/api/app/collections/${UCOL}`, {
            app: UNTOUCHED_APP,
            version: "production",
            body: { display_name: renamed, fields: cur.fields ?? [] },
        });
        data(res, "rename untouched collection in production");

        // an edit in APP (so the wholesale deploy actually writes APP)
        await updateCollectionFields(APP, VERSION, NOTES, [{ name: "untouched_probe", type: "string" }]);
        await pollDryRun(
            versionId,
            (d) =>
                (d.edits ?? []).some(
                    (e: any) => e.app === APP && e.target === "alcedocore_fields" && e.action === "item_created",
                ),
            25000,
            "untouched_probe field edit",
        );

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        assert.strictEqual(data(dep, "deploy for untouched-app pin").deployed, true);

        // APP was written...
        assert.ok(
            (await getCollection(APP, "production", NOTES)).fields.some((f: any) => f.name === "untouched_probe"),
            "APP must have been written",
        );
        // ...but the untouched app kept its production-side change
        const prodU = await getCollection(UNTOUCHED_APP, "production", UCOL);
        assert.strictEqual(prodU.display_name, renamed, "prod-side rename of an untouched app must survive");
        // and the version side still has the pre-rename value
        const verU = await getCollection(UNTOUCHED_APP, VERSION, UCOL);
        assert.notStrictEqual(verU.display_name, renamed);
    });

    it("deploys a cross-app relation in one wholesale run, related app first (topo order)", async () => {
        // XAPP gets a cross-app M:1 field pointing at YAPP's collection;
        // YAPP gets its own edit, so both apps are written by the deploy and
        // the deploy must order YAPP before XAPP (XAPP's version schema has
        // an FK into YAPP's version schema)
        await updateCollectionFields(XAPP, VERSION, XCOL, [
            {
                name: "yref",
                type: "relationship",
                relationship_type: "many_to_one",
                related_collection: YCOL,
                related_app: YAPP,
            },
        ]);
        await updateCollectionFields(YAPP, VERSION, YCOL, [{ name: "ynote", type: "string" }]);

        await pollDryRun(
            versionId,
            (d) =>
                (d.edits ?? []).some((e: any) => e.app === XAPP && e.target === "alcedocore_fields") &&
                (d.edits ?? []).some((e: any) => e.app === YAPP && e.target === "alcedocore_fields"),
            25000,
            "cross-app field edits in XAPP and YAPP",
        );

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 400));
        const d = data(dep, "cross-app deploy");
        assert.strictEqual(d.deployed, true);
        const written = (d.apps ?? []).map((a: any) => a.app);
        assert.ok(written.includes(XAPP) && written.includes(YAPP), `both apps written: ${JSON.stringify(d.apps)}`);
        // YAPP (the dependency) must be written before XAPP
        assert.ok(
            written.indexOf(YAPP) < written.indexOf(XAPP),
            `dependency ${YAPP} must deploy before ${XAPP}: ${JSON.stringify(written)}`,
        );

        const prodX = await getCollection(XAPP, "production", XCOL);
        const yref = prodX.fields.find((f: any) => f.name === "yref");
        assert.ok(yref, `prod ${XCOL} must have yref: ${JSON.stringify(prodX.fields)}`);
        assert.strictEqual(yref.related_app, YAPP);
        assert.strictEqual(yref.related_collection, YCOL);
        assert.ok(
            (await getCollection(YAPP, "production", YCOL)).fields.some((f: any) => f.name === "ynote"),
            "prod YCOL must have ynote",
        );

        // functional proof: the recreated FK points at the PRODUCTION schema of YAPP
        const yItem = await call("POST", `/api/app/items/${YCOL}`, {
            app: YAPP,
            version: "production",
            body: { title: "y1" },
        });
        const yId = data(yItem, "create prod YCOL item").created[0].id;
        const xItem = await call("POST", `/api/app/items/${XCOL}`, {
            app: XAPP,
            version: "production",
            body: { title: "x1", yref: yId },
        });
        assert.strictEqual(xItem.status, 200, `cross-app insert in production must work: ${JSON.stringify(xItem.json).slice(0, 300)}`);
    });

    it("re-deploy with a changed plugin version re-provisions the production install (still one install)", async () => {
        // same slug on the version's app-version, bumped plugin_version
        const res = await call("POST", "/api/platform/plugins/deploy", {
            body: {
                slug: "hello-world",
                plugin_version: "1.0.1",
                image: "localhost:5000/hello-world:1.0.0",
                registry_id: 1,
                app_version_id: versionLinkId,
                scope: "app",
                env: {},
                granted_scopes: ["kv.all"],
            },
        });
        assert.strictEqual(res.status, 200, `plugin re-deploy: ${JSON.stringify(res.json).slice(0, 300)}`);

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        const d = data(dep, "plugin version-change deploy");
        assert.strictEqual(d.deployed, true);
        assert.ok(
            (d.plugins ?? []).some((p: any) => p.runtime_provisioned === true),
            `expected a re-provisioned install: ${JSON.stringify(d.plugins)}`,
        );

        const plugins = data(await call("GET", "/api/platform/plugins"), "plugins").plugins;
        const hw = plugins.find((p: any) => p.slug === "hello-world");
        const prodInstalls = (hw.installations ?? []).filter((i: any) => i.app_version_id === prodLinkId);
        assert.strictEqual(prodInstalls.length, 1, "still exactly one production install");
        assert.strictEqual(prodInstalls[0].plugin_version, "1.0.1", "prod install must carry the new version");
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

    it("refuses a relation-target change on an existing field (409 unsupported even with overwrite)", async () => {
        // point the version's `parent` field (deployed to production in the
        // first test) at NOTESX instead of NOTES — a deploy cannot retarget
        // the physical FK in production
        const list = await call(
            "GET",
            "/api/app/items/alcedocore_fields?filter%5Bapi_name%5D%5B_eq%5D=parent&limit=10",
            { app: APP, version: VERSION },
        );
        const rows = data(list, "list parent field rows").data ?? [];
        const parent = rows.find((r: any) => r.api_name === "parent");
        assert.ok(parent, `parent field row must exist in the version: ${JSON.stringify(rows)}`);
        const patch = await call(
            "PATCH",
            "/api/app/items/alcedocore_fields?filter%5Bapi_name%5D%5B_eq%5D=parent",
            {
                app: APP,
                version: VERSION,
                body: { options: { ...(parent.options ?? {}), related_collection: NOTESX } },
            },
        );
        data(patch, "retarget parent field in version");

        const dry = await pollDryRun(
            versionId,
            (d) =>
                (d.unsupported ?? []).some(
                    (u: any) => u.type === "field_type_change" && /relation target/.test(u.detail ?? ""),
                ),
            25000,
            "relation-target unsupported",
        );
        assert.ok(dry.unsupported.some((u: any) => /relation target/.test(u.detail ?? "")));

        const dep = await deploy(versionId, { on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 409, `expected 409, got ${dep.status}`);
        assert.ok(
            (dep.json.data?.unsupported ?? []).some(
                (u: any) => u.type === "field_type_change" && /relation target/.test(u.detail ?? ""),
            ),
            JSON.stringify(dep.json.data?.unsupported),
        );

        const prod = await getCollection(APP, "production", NOTES);
        const prodParent = prod.fields.find((f: any) => f.name === "parent");
        assert.strictEqual(
            prodParent?.related_collection,
            NOTES,
            "production relation target must be unchanged",
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

    it("rejects non-admin authenticated users with 403", async () => {
        const email = `e2e_user_${SUFFIX}@example.com`;
        const password = "Passw0rd!123";
        const created = await call("POST", "/api/platform/users", {
            body: { email, password, display_name: "e2e non-admin", is_admin: false },
        });
        const userId = data(created, "create non-admin user").id;
        assert.ok(userId, "user id expected");

        const userCookie = await login(email, password);
        const cases: [string, string, unknown][] = [
            ["GET", "/api/platform/versions", undefined],
            ["POST", `/api/platform/versions/${versionId}/deploy`, { dry_run: true }],
            ["POST", `/api/platform/versions/${versionId}/revert`, {}],
            ["DELETE", `/api/platform/versions/${versionId}`, undefined],
        ];
        for (const [method, path, body] of cases) {
            const res = await call(method, path, { cookieOverride: userCookie, body });
            assert.strictEqual(
                res.status,
                403,
                `${method} ${path} must be 403 for a non-admin user, got ${res.status}: ${JSON.stringify(res.json).slice(0, 200)}`,
            );
        }

        await call("DELETE", `/api/platform/users/${userId}`);
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

    it("revert applies user-item inverses to production rows (pinned quirk) and carries the no-backup warning", async () => {
        // isolated version so the refusal tests' pending unsupported edits
        // (on versionId) cannot interfere
        const ver = await call("POST", "/api/platform/versions", { body: { version_name: VERSION3 } });
        version3Id = data(ver, "create third version").id;

        // an applicable system edit, so the wholesale deploy writes APP and
        // records the collection-log edit as "handled"
        await updateCollectionFields(APP, VERSION3, NOTES, [{ name: "revert_probe", type: "string" }]);
        const created = await call("POST", `/api/app/items/${NOTES}`, {
            app: APP,
            version: VERSION3,
            body: { title: "v3 item" },
        });
        const id = String(data(created, "create item in v3").created[0].id);
        // the same item id exists in production (created there directly)
        const prodSame = await call("POST", `/api/app/items/${NOTES}`, {
            app: APP,
            version: "production",
            body: { id, title: "prod-own-row" },
        });
        data(prodSame, "create same-id row in production");

        await pollDryRun(
            version3Id,
            (d) =>
                (d.edits ?? []).some(
                    (e: any) => e.source === "collection" && e.action === "item_created" && e.collection === NOTES,
                ),
            25000,
            "v3 item_created edit",
        );

        const dep = await deploy(version3Id, { on_conflict: "overwrite" });
        assert.strictEqual(dep.status, 200, JSON.stringify(dep.json).slice(0, 300));
        const depData = data(dep, "v3 deploy");
        assert.strictEqual(depData.deployed, true);
        assert.match(depData.warning ?? "", /No data backup/, "deploy response must carry the no-backup warning");

        // user item rows are not copied: the prod row keeps its own value
        const before = data(
            await call("GET", `/api/app/items/${NOTES}/${id}`, { app: APP, version: "production" }),
            "prod same-id row before revert",
        );
        assert.strictEqual(before.title, "prod-own-row");

        const rev = await call("POST", `/api/platform/versions/${version3Id}/revert`);
        assert.strictEqual(rev.status, 200, JSON.stringify(rev.json).slice(0, 300));
        const revData = data(rev, "v3 revert");
        assert.match(revData.warning ?? "", /No data backup/, "revert response must carry the no-backup warning");
        assert.ok(
            (revData.reverted ?? []).some(
                (r: any) => r.source === "collection" && r.action === "item_created" && r.collection === NOTES,
            ),
            `item_created must be reverted: ${JSON.stringify(revData.reverted)}`,
        );

        // PINNED QUIRK: the inverse of item_created is DELETE ... WHERE pk = item_id
        // in the PRODUCTION schema — the prod row with that id is deleted even
        // though the deploy never copied it (wholesale only marks the edit handled)
        const after = await call("GET", `/api/app/items/${NOTES}/${id}`, { app: APP, version: "production" });
        assert.strictEqual(after.status, 404, `prod row must be gone after revert, got ${after.status}: ${JSON.stringify(after.json).slice(0, 150)}`);
    });
});
