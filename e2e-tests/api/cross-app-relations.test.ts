/**
 * corev2 cross-app relationship test (same version).
 *
 * Proves that a relationship in app A can target a collection in app B
 * (same version) for reads AND writes:
 *  - collection metadata round-trips `related_app`
 *  - cross-app M:1 read expansion (dotted `fields`)
 *  - cross-app M:1 nested create / update (FK unchanged) / unlink
 *  - cross-app 1:M nested create / update / delete
 *  - validation rejects a relationship to an app not on the version
 *
 * Run (corev2 must be running):
 *   cd e2e-tests
 *   node --test ./api/cross-app-relations.test.ts
 *
 * Env overrides: CORE_URL, TEST_APP, TEST_VERSION, ADMIN_EMAIL, ADMIN_PASSWORD.
 */
import { describe, it, before, after } from "node:test";
import assert from "node:assert";

const API = process.env.CORE_URL ?? "http://localhost:8080";
const APP_A = process.env.TEST_APP ?? "test";
const VERSION = process.env.TEST_VERSION ?? "production";
const EMAIL = process.env.ADMIN_EMAIL ?? "admin@alcedo.dev";
const PASSWORD = process.env.ADMIN_PASSWORD ?? "admin123!";

const SUFFIX = Math.random().toString(36).slice(2, 8);
const APP_B = `xapp_${SUFFIX}`;
const POSTS = `xposts_${SUFFIX}`;
const ARTICLES = `xarticles_${SUFFIX}`;

let cookie = "";
let appBId: number | null = null;

interface CallOptions {
    cookie?: string;
    app?: string;
    version?: string;
    body?: unknown;
    query?: Record<string, string>;
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
    if (!match) throw new Error(`No session cookie: ${setCookie}`);
    return `alcedo_session=${match[1]}`;
}

async function call(
    method: string,
    path: string,
    opts: CallOptions = {},
): Promise<{ status: number; json: any }> {
    let url = `${API}${path}`;
    if (opts.query) url += `?${new URLSearchParams(opts.query).toString()}`;
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    const c = opts.cookie ?? cookie;
    if (c) headers["Cookie"] = c;
    if (opts.app) headers["X-App"] = opts.app;
    if (opts.version) headers["X-Version"] = opts.version;
    const res = await fetch(url, {
        method,
        headers,
        body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
        redirect: "manual",
    });
    const text = await res.text();
    let json: any;
    try {
        json = JSON.parse(text);
    } catch {
        json = text;
    }
    return { status: res.status, json };
}

async function ok(method: string, path: string, opts: CallOptions = {}): Promise<any> {
    const res = await call(method, path, opts);
    assert.strictEqual(
        res.status,
        200,
        `${method} ${path} -> ${res.status}: ${JSON.stringify(res.json)}`,
    );
    assert.strictEqual(
        res.json.status,
        "success",
        `${method} ${path} failed: ${JSON.stringify(res.json)}`,
    );
    return res.json.data;
}

/** App A (base) request. */
const A = { app: APP_A, version: VERSION };
/** App B (cross) request. */
const B = { app: APP_B, version: VERSION };

before(async () => {
    cookie = await login();

    // Create a second app attached to the same version.
    const app = await ok("POST", "/api/platform/apps", {
        body: { name: `Cross App ${SUFFIX}`, api_name: APP_B, version: VERSION },
    });
    appBId = app.id;

    // A collection in app B.
    await ok("POST", "/api/app/collections", {
        ...B,
        body: {
            name: POSTS,
            display_name: "Posts",
            fields: [
                { name: "title", type: "string", required: true, ordinal_position: 1 },
            ],
        },
    });

    // A collection in app A with a cross-app M:1 to app B.
    await ok("POST", "/api/app/collections", {
        ...A,
        body: {
            name: ARTICLES,
            display_name: "Articles",
            fields: [
                { name: "headline", type: "string", required: true, ordinal_position: 1 },
                {
                    name: "post",
                    type: "relationship",
                    relationship_type: "many_to_one",
                    related_collection: POSTS,
                    related_app: APP_B,
                    ordinal_position: 2,
                },
            ],
        },
    });

    // Add the virtual 1:M side on app B's posts, targeting app A's articles.
    await ok("PUT", `/api/app/collections/${POSTS}`, {
        ...B,
        body: {
            fields: [
                {
                    name: "articles",
                    type: "relationship",
                    relationship_type: "one_to_many",
                    related_collection: ARTICLES,
                    related_app: APP_A,
                    ordinal_position: 2,
                },
            ],
            removed_fields: [],
        },
    });
});

after(async () => {
    try {
        await call("DELETE", `/api/app/collections/${ARTICLES}`, { ...A });
    } catch {
        /* best effort */
    }
    try {
        await call("DELETE", `/api/app/collections/${POSTS}`, { ...B });
    } catch {
        /* best effort */
    }
    if (appBId !== null) {
        try {
            await call("DELETE", `/api/platform/apps/${appBId}`);
        } catch {
            /* best effort */
        }
    }
});

describe("corev2 cross-app relationships", () => {
    it("round-trips related_app in collection metadata", async () => {
        const collection = await ok("GET", `/api/app/collections/${ARTICLES}`, { ...A });
        const post = collection.fields.find((f: any) => f.name === "post");
        assert.strictEqual(post.related_collection, POSTS);
        assert.strictEqual(post.related_app, APP_B);
    });

    it("reads an expanded cross-app M:1 relation", async () => {
        const post = await ok("POST", `/api/app/items/${POSTS}`, {
            ...B,
            body: { title: "Cross Post" },
        });
        const postId = post.created[0].id;

        await ok("POST", `/api/app/items/${ARTICLES}`, {
            ...A,
            body: { headline: "Linked", post: postId },
        });

        const data = await ok("GET", `/api/app/items/${ARTICLES}`, {
            ...A,
            query: {
                filter: JSON.stringify({ _and: [{ headline: { _eq: "Linked" } }] }),
                fields: JSON.stringify(["*", "post.*"]),
            },
        });
        assert.strictEqual(data.data.length, 1);
        assert.strictEqual(typeof data.data[0].post, "object");
        assert.strictEqual(data.data[0].post.id, postId);
        assert.strictEqual(data.data[0].post.title, "Cross Post");
    });

    it("creates a nested cross-app M:1 record", async () => {
        const data = await ok("POST", `/api/app/items/${ARTICLES}`, {
            ...A,
            body: { headline: "Nested", post: { title: "Nested Post" } },
        });
        const article = data.created[0];
        assert.ok(article.post, "article has the related FK");

        const parent = await ok("GET", `/api/app/items/${POSTS}/${article.post}`, { ...B });
        assert.strictEqual(parent.title, "Nested Post");
    });

    it("updates a related cross-app record without changing the FK", async () => {
        const post = await ok("POST", `/api/app/items/${POSTS}`, {
            ...B,
            body: { title: "FK Post" },
        });
        const postId = post.created[0].id;
        const article = await ok("POST", `/api/app/items/${ARTICLES}`, {
            ...A,
            body: { headline: "FK Article", post: postId },
        });
        const articleId = article.created[0].id;

        const updated = await ok("PATCH", `/api/app/items/${ARTICLES}/${articleId}`, {
            ...A,
            body: { post: { id: postId, title: "FK Post Updated" } },
        });
        assert.strictEqual(updated.post, postId, "FK unchanged");

        const reread = await ok("GET", `/api/app/items/${POSTS}/${postId}`, { ...B });
        assert.strictEqual(reread.title, "FK Post Updated");
    });

    it("unlinks a cross-app M:1 relation", async () => {
        const post = await ok("POST", `/api/app/items/${POSTS}`, {
            ...B,
            body: { title: "Unlink Post" },
        });
        const postId = post.created[0].id;
        const article = await ok("POST", `/api/app/items/${ARTICLES}`, {
            ...A,
            body: { headline: "Unlink Article", post: postId },
        });
        const articleId = article.created[0].id;

        const updated = await ok("PATCH", `/api/app/items/${ARTICLES}/${articleId}`, {
            ...A,
            body: { post: null },
        });
        assert.strictEqual(updated.post, null);
    });

    it("creates/updates/deletes cross-app 1:M children", async () => {
        const post = await ok("POST", `/api/app/items/${POSTS}`, {
            ...B,
            body: { title: "O2M Post" },
        });
        const postId = post.created[0].id;

        // create two children in app A
        await ok("PATCH", `/api/app/items/${POSTS}/${postId}`, {
            ...B,
            body: { articles: { create: [{ headline: "OA" }, { headline: "OB" }] } },
        });
        let children = await ok("GET", `/api/app/items/${ARTICLES}`, {
            ...A,
            query: {
                filter: JSON.stringify({ _and: [{ post: { _eq: postId } }] }),
            },
        });
        assert.strictEqual(children.total, 2);

        const a = children.data.find((c: any) => c.headline === "OA");
        const b = children.data.find((c: any) => c.headline === "OB");

        // update A + delete B
        await ok("PATCH", `/api/app/items/${POSTS}/${postId}`, {
            ...B,
            body: {
                articles: {
                    update: [{ id: a.id, headline: "OA Updated" }],
                    delete: [b.id],
                },
            },
        });

        const updatedA = await ok("GET", `/api/app/items/${ARTICLES}/${a.id}`, { ...A });
        assert.strictEqual(updatedA.headline, "OA Updated");

        children = await ok("GET", `/api/app/items/${ARTICLES}`, {
            ...A,
            query: {
                filter: JSON.stringify({ _and: [{ post: { _eq: postId } }] }),
            },
        });
        assert.strictEqual(children.total, 1);
    });

    it("rejects a relationship to an app not attached to the version", async () => {
        const res = await call("POST", "/api/app/collections", {
            ...A,
            body: {
                name: `badrel_${SUFFIX}`,
                fields: [
                    {
                        name: "thing",
                        type: "relationship",
                        relationship_type: "many_to_one",
                        related_collection: POSTS,
                        related_app: "does_not_exist_app",
                    },
                ],
            },
        });
        assert.notStrictEqual(res.status, 200, "should not succeed");
        assert.ok(
            JSON.stringify(res.json).includes("not attached to version"),
            `expected an 'not attached to version' error, got ${JSON.stringify(res.json)}`,
        );
    });
});
