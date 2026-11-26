/**
 * Version-branches e2e test (HTTP API, no SDK).
 *
 * Drives the full version-branch lifecycle against a running core:
 *  1. create an app linked to `production` + a collection + an item
 *  2. create a new version (clones production apps into it)
 *  3. make a schema edit in the version (new collection)
 *  4. dry-run deploy -> nothing written; real deploy -> edit lands in production
 *  5. production data survives the deploy; version is marked deployed
 *  6. revert -> edit removed from production again
 *  7. delete version -> links + schemas gone, production untouched
 *  8. negative: deleting the `production` version is refused
 *
 * Run (core must be running):
 *   cd e2e-tests
 *   CORE_URL=http://localhost npx tsx --test ./api/version-branches.test.ts
 *
 * Env overrides: CORE_URL, ADMIN_EMAIL, ADMIN_PASSWORD.
 */
import { describe, it, before, after } from "node:test";
import assert from "node:assert";

const API = process.env.CORE_URL ?? "http://localhost:8080";
const EMAIL = process.env.ADMIN_EMAIL ?? "admin@alcedo.dev";
const PASSWORD = process.env.ADMIN_PASSWORD ?? "admin123!";

// Never send the admin password or session cookie over cleartext HTTP to a
// non-loopback host: allow http only for loopback test servers.
function assertSafeApiBase(baseUrl: string): void {
    const u = new URL(baseUrl);
    const loopback =
        u.hostname === "localhost" ||
        u.hostname.endsWith(".localhost") ||
        u.hostname === "::1" ||
        u.hostname === "[::1]" ||
        u.hostname.startsWith("127.");
    if (u.protocol === "https:" || (u.protocol === "http:" && loopback)) return;
    throw new Error(`CORE_URL must use https (http allowed only for loopback), got: ${baseUrl}`);
}
assertSafeApiBase(API);

const SUFFIX = Math.random().toString(36).slice(2, 8);
const APP = `e2eapp_${SUFFIX}`;
const VERSION = `e2e_${SUFFIX}`;
const NOTES = `e2e_notes_${SUFFIX}`;
const NOTES2 = `e2e_notes2_${SUFFIX}`;

let cookie = "";
let appId = 0;
let versionId = 0;
let productionVersionId = 0;
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
}

async function call(
    method: string,
    path: string,
    opts: CallOptions = {},
): Promise<{ status: number; json: any }> {
    const res = await fetch(`${API}${path}`, {
        method,
        headers: headers(cookie, opts.app, opts.version),
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

/** JSend success unwrap; asserts the envelope is `success`. */
function data(res: { status: number; json: any }, what: string): any {
    assert.strictEqual(res.json?.status, "success", `${what}: expected JSend success, got ${JSON.stringify(res.json).slice(0, 300)}`);
    return res.json.data;
}

async function collectionNames(app: string, version: string): Promise<string[]> {
    const res = await call("GET", "/api/app/collections", { app, version });
    const cols = data(res, "list collections");
    const list = Array.isArray(cols) ? cols : cols.collections ?? cols.data ?? [];
    return list.map((c: any) => c.table ?? c.name ?? c.api_name);
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

describe("version branches e2e", () => {
    before(async () => {
        cookie = await login();
    });

    after(async () => {
        // Best-effort cleanup: delete the version (if still there) and the app.
        if (versionId) {
            await call("DELETE", `/api/platform/versions/${versionId}`);
        }
        if (appId) {
            await call("DELETE", `/api/platform/apps/${appId}`);
        }
    });

    it("creates an app on production with a collection and an item", async () => {
        const app = await call("POST", "/api/platform/apps", {
            body: { name: `E2E ${SUFFIX}`, api_name: APP, version: "production" },
        });
        appId = data(app, "create app").id;
        assert.ok(appId > 0);

        const col = await call("POST", "/api/app/collections", {
            app: APP,
            version: "production",
            body: {
                name: NOTES,
                fields: [{ name: "title", type: "String", required: true }],
            },
        });
        data(col, "create collection");

        const item = await call("POST", `/api/app/items/${NOTES}`, {
            app: APP,
            version: "production",
            body: { title: "hello production" },
        });
        // create-item response shape: { created: [ {...} ] }
        itemId = data(item, "create item").created[0].id;
        assert.ok(itemId);

        const versions = data(await call("GET", "/api/platform/versions"), "list versions");
        const prod = versions.find((v: any) => v.version_name === "production");
        assert.ok(prod, "production version must exist");
        productionVersionId = prod.id;
    });

    it("creates a new version that inherits the production apps", async () => {
        const res = await call("POST", "/api/platform/versions", {
            body: { version_name: VERSION },
        });
        versionId = data(res, "create version").id;
        assert.ok(versionId > 0);

        const links = data(await call("GET", "/api/platform/app-versions"), "app-versions");
        const link = links.find(
            (l: any) => l.api_name === APP && String(l.version_id) === String(versionId),
        );
        assert.ok(link, `expected ${APP} linked to version ${versionId} (${VERSION})`);
    });

    it("makes a schema edit inside the version only", async () => {
        const col = await call("POST", "/api/app/collections", {
            app: APP,
            version: VERSION,
            body: {
                name: NOTES2,
                fields: [{ name: "body", type: "String" }],
            },
        });
        data(col, "create collection in version");

        // The edit must not be visible in production yet.
        const prodCols = await collectionNames(APP, "production");
        assert.ok(!prodCols.includes(NOTES2), `production must not have ${NOTES2} before deploy`);
        const verCols = await collectionNames(APP, VERSION);
        assert.ok(verCols.includes(NOTES2), `version must have ${NOTES2}`);
    });

    it("dry-run deploy reports edits but writes nothing", async () => {
        // The activity log is written through an async channel, so the edit may
        // take a moment to become visible to deploy; poll the dry run until it
        // shows up (or time out).
        let d: any = null;
        const deadline = Date.now() + 20000;
        while (Date.now() < deadline) {
            const res = await call("POST", `/api/platform/versions/${versionId}/deploy`, {
                body: { dry_run: true },
            });
            assert.strictEqual(res.status, 200, JSON.stringify(res.json).slice(0, 300));
            d = data(res, "dry-run deploy");
            if (Array.isArray(d.edits) && d.edits.length >= 1) break;
            await new Promise((r) => setTimeout(r, 500));
        }
        assert.ok(d, "dry run never ran");
        assert.strictEqual(d.deployed, false);
        assert.ok(Array.isArray(d.edits) && d.edits.length >= 1, "dry run must list edits");
        assert.ok(
            d.edits.some((e: any) => e.action === "collection_created"),
            `expected a collection_created edit: ${JSON.stringify(d.edits)}`,
        );

        const prodCols = await collectionNames(APP, "production");
        assert.ok(!prodCols.includes(NOTES2), `dry run must not write: production still has ${NOTES2}?`);
    });

    it("refuses deploy with on_conflict=fail when the new collection is missing in production", async () => {
        const res = await call("POST", `/api/platform/versions/${versionId}/deploy`, {
            body: { on_conflict: "fail" },
        });
        assert.strictEqual(res.status, 409, `expected 409, got ${res.status} ${JSON.stringify(res.json).slice(0, 200)}`);
        assert.strictEqual(res.json?.status, "fail");
        const conflicts = res.json.data?.conflicts ?? [];
        assert.ok(
            conflicts.some(
                (c: any) => c.type === "relation_missing_in_prod" && c.collection === NOTES2,
            ),
            `expected relation_missing_in_prod conflict for ${NOTES2}: ${JSON.stringify(conflicts)}`,
        );
        assert.strictEqual(res.json.data?.deployed, false);

        const prodCols = await collectionNames(APP, "production");
        assert.ok(!prodCols.includes(NOTES2), "refused deploy must not write to production");
    });

    it("deploys the version into production (data preserved, marked deployed)", async () => {
        const res = await call("POST", `/api/platform/versions/${versionId}/deploy`, {
            body: { on_conflict: "overwrite" },
        });
        assert.strictEqual(res.status, 200, JSON.stringify(res.json).slice(0, 300));
        const d = data(res, "deploy");
        assert.strictEqual(d.deployed, true);

        const prodCols = await collectionNames(APP, "production");
        assert.ok(prodCols.includes(NOTES2), `production must have ${NOTES2} after deploy`);

        // Pre-existing production item must survive the deploy.
        const item = await call("GET", `/api/app/items/${NOTES}/${itemId}`, {
            app: APP,
            version: "production",
        });
        assert.strictEqual(data(item, "get item after deploy").title, "hello production");

        // The applied edit must be recorded as deployed (version_deployed
        // marker, written through the async activity-log channel): a fresh
        // dry run must no longer list the collection_created edit as pending.
        let pending = true;
        const deadline = Date.now() + 20000;
        while (Date.now() < deadline) {
            const res = await call("POST", `/api/platform/versions/${versionId}/deploy`, {
                body: { dry_run: true },
            });
            const d = data(res, "dry-run after deploy");
            const stillPending = (d.edits ?? []).some(
                (e: any) => e.action === "collection_created",
            );
            if (!stillPending) {
                pending = false;
                break;
            }
            await new Promise((r) => setTimeout(r, 500));
        }
        assert.ok(!pending, "collection_created edit must be marked applied after deploy");
    });

    it("reverts the version, removing the deployed edit from production", async () => {
        const res = await call("POST", `/api/platform/versions/${versionId}/revert`);
        assert.strictEqual(res.status, 200, JSON.stringify(res.json).slice(0, 300));
        data(res, "revert");

        const prodCols = await collectionNames(APP, "production");
        assert.ok(!prodCols.includes(NOTES2), `revert must remove ${NOTES2} from production`);

        const item = await call("GET", `/api/app/items/${NOTES}/${itemId}`, {
            app: APP,
            version: "production",
        });
        data(item, "get item after revert");
    });

    it("deletes the version and refuses to delete production", async () => {
        const prod = await call("DELETE", `/api/platform/versions/${productionVersionId}`);
        assert.ok(prod.status >= 400, `deleting production must fail, got ${prod.status}`);
        assert.notStrictEqual(prod.json?.status, "success");

        const del = await call("DELETE", `/api/platform/versions/${versionId}`);
        assert.strictEqual(del.status, 200, JSON.stringify(del.json).slice(0, 300));
        data(del, "delete version");

        const versions = data(await call("GET", "/api/platform/versions"), "list versions");
        assert.ok(!versions.some((v: any) => v.id === versionId), "version must be gone");

        const links = data(await call("GET", "/api/platform/app-versions"), "app-versions");
        assert.ok(
            !links.some((l: any) => String(l.version_id) === String(versionId)),
            "app-version link must be gone",
        );

        // The version schema is dropped: using it must now fail.
        const gone = await call("GET", "/api/app/collections", { app: APP, version: VERSION });
        assert.notStrictEqual(gone.json?.status, "success", "deleted version schema must not serve collections");

        // Production is untouched.
        const prodCols = await collectionNames(APP, "production");
        assert.ok(prodCols.includes(NOTES), "production collection must survive version deletion");

        versionId = 0; // already deleted; skip in after()
    });
});
