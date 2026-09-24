/**
 * corev2 advanced relationships test.
 *
 * Covers three things the basic relationship suites do not:
 *  1. A three-hop M:1 filter inside one app
 *     (`notes -> items -> orders -> customers`, `customer.full_name`).
 *  2. A multi-hop M:1 filter where one hop crosses into another app
 *     (`comments -> articles -> posts`, posts lives in app B).
 *  3. `related_app` round-trips on a layout section (relational section).
 *
 * Run (corev2 must be running):
 *   cd e2e-tests
 *   node --test ./api/relations-advanced.test.ts
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
const APP_B = `xrel_${SUFFIX}`;
const APP_C = `xrelc_${SUFFIX}`;

// Same-app chain: note -> item -> order -> customer (M:1 each).
const CUSTOMERS = `rcustomers_${SUFFIX}`;
const ORDERS = `rorders_${SUFFIX}`;
const ITEMS = `ritems_${SUFFIX}`;
const NOTES = `rnotes_${SUFFIX}`;

// Cross-app chain: comment -> article -> post (post lives in app B).
const POSTS = `rposts_${SUFFIX}`;
const ARTICLES = `rarticles_${SUFFIX}`;
const COMMENTS = `rcomments_${SUFFIX}`;

// Three-hop cross-app chain: note -> item -> order (A) -> customer (B).
const B_CUSTOMERS = `r3customers_${SUFFIX}`;
const A_ORDERS = `r3orders_${SUFFIX}`;
const A_ITEMS = `r3items_${SUFFIX}`;
const A_NOTES = `r3notes_${SUFFIX}`;

// Retarget chain: refs (A) -> targets (B, later C).
const TARGETS = `rtargets_${SUFFIX}`;
const REFS = `rrefs_${SUFFIX}`;

let cookie = "";
let appBId: number | null = null;
let appCId: number | null = null;
const createdCollections: { name: string; app: string }[] = [];

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
/** App C (second cross app, used for retargeting). */
const C = { app: APP_C, version: VERSION };

async function createCollection(name: string, app: string, fields: any[]): Promise<void> {
    await ok("POST", "/api/app/collections", {
        app,
        version: VERSION,
        body: { name, display_name: name, fields },
    });
    createdCollections.push({ name, app });
}

function relationship(
    name: string,
    relationship_type: "many_to_one" | "one_to_many",
    related_collection: string,
    ordinal_position: number,
    related_app?: string,
): any {
    const field: any = {
        name,
        type: "relationship",
        relationship_type,
        related_collection,
        ordinal_position,
    };
    if (related_app) field.related_app = related_app;
    return field;
}

/** Adds a single field to an existing collection (e.g. a reverse 1:M). */
async function addField(name: string, app: string, field: any): Promise<void> {
    await ok("PUT", `/api/app/collections/${name}`, {
        app,
        version: VERSION,
        body: { fields: [field], removed_fields: [] },
    });
}

before(async () => {
    cookie = await login();

    // Create a second app attached to the same version.
    const app = await ok("POST", "/api/platform/apps", {
        body: { name: `XRel App ${SUFFIX}`, api_name: APP_B, version: VERSION },
    });
    appBId = app.id;

    // Create a third app attached to the same version (retarget target).
    const appC = await ok("POST", "/api/platform/apps", {
        body: { name: `XRel App C ${SUFFIX}`, api_name: APP_C, version: VERSION },
    });
    appCId = appC.id;

    // --- Same-app three-hop chain (parents first) ---
    await createCollection(CUSTOMERS, APP_A, [
        { name: "full_name", type: "string", required: true, ordinal_position: 1 },
    ]);
    await createCollection(ORDERS, APP_A, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        relationship("customer", "many_to_one", CUSTOMERS, 2),
    ]);
    await createCollection(ITEMS, APP_A, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        relationship("order", "many_to_one", ORDERS, 2),
    ]);
    await createCollection(NOTES, APP_A, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        relationship("item", "many_to_one", ITEMS, 2),
    ]);

    // --- Cross-app chain: posts (B) <- articles (A) <- comments (A) ---
    await createCollection(POSTS, APP_B, [
        { name: "title", type: "string", required: true, ordinal_position: 1 },
    ]);
    await createCollection(ARTICLES, APP_A, [
        { name: "headline", type: "string", required: true, ordinal_position: 1 },
        relationship("post", "many_to_one", POSTS, 2, APP_B),
    ]);
    await createCollection(COMMENTS, APP_A, [
        { name: "body", type: "string", required: true, ordinal_position: 1 },
        relationship("article", "many_to_one", ARTICLES, 2),
    ]);

    // --- Three-hop cross-app chain: note -> item -> order (A) -> customer (B) ---
    await createCollection(B_CUSTOMERS, APP_B, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
    ]);
    await createCollection(A_ORDERS, APP_A, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        relationship("customer", "many_to_one", B_CUSTOMERS, 2, APP_B),
    ]);
    await createCollection(A_ITEMS, APP_A, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        relationship("order", "many_to_one", A_ORDERS, 2),
    ]);
    await createCollection(A_NOTES, APP_A, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        relationship("item", "many_to_one", A_ITEMS, 2),
    ]);

    // --- Retarget chain: refs (A) -> targets (B), later retargeted to C ---
    await createCollection(TARGETS, APP_B, [
        { name: "label", type: "string", required: true, ordinal_position: 1 },
    ]);
    await createCollection(TARGETS, APP_C, [
        { name: "label", type: "string", required: true, ordinal_position: 1 },
    ]);
    await createCollection(REFS, APP_A, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        relationship("target", "many_to_one", TARGETS, 2, APP_B),
    ]);

    // --- Reverse 1:M fields used by the nested-filter tests ---
    await addField(CUSTOMERS, APP_A, relationship("orders", "one_to_many", ORDERS, 2));
    await addField(ORDERS, APP_A, relationship("items", "one_to_many", ITEMS, 3));
    await addField(
        B_CUSTOMERS,
        APP_B,
        relationship("orders", "one_to_many", A_ORDERS, 2, APP_A),
    );
});

after(async () => {
    // Children before parents (reverse creation order).
    for (const { name, app } of [...createdCollections].reverse()) {
        try {
            await call("DELETE", `/api/app/collections/${name}`, {
                app,
                version: VERSION,
            });
        } catch {
            /* best effort */
        }
    }
    if (appCId !== null) {
        try {
            await call("DELETE", `/api/platform/apps/${appCId}`);
        } catch {
            /* best effort */
        }
    }
    if (appBId !== null) {
        try {
            await call("DELETE", `/api/platform/apps/${appBId}`);
        } catch {
            /* best effort */
        }
    }
});

describe("corev2 advanced relationships", () => {
    it("filters across three same-app M:1 hops", async () => {
        const alice = await ok("POST", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            body: { full_name: "Alice" },
        });
        const aliceId = alice.created[0].id;
        const order = await ok("POST", `/api/app/items/${ORDERS}`, {
            ...A,
            body: { name: "Order Alice", customer: aliceId },
        });
        const orderId = order.created[0].id;
        const item = await ok("POST", `/api/app/items/${ITEMS}`, {
            ...A,
            body: { name: "Item Alice", order: orderId },
        });
        const itemId = item.created[0].id;
        await ok("POST", `/api/app/items/${NOTES}`, {
            ...A,
            body: { name: "Note Alice", item: itemId },
        });

        // A decoy chain that must not match "Alice".
        const carol = await ok("POST", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            body: { full_name: "Carol" },
        });
        const carolId = carol.created[0].id;
        const carolOrder = await ok("POST", `/api/app/items/${ORDERS}`, {
            ...A,
            body: { name: "Order Carol", customer: carolId },
        });
        const carolOrderId = carolOrder.created[0].id;
        const carolItem = await ok("POST", `/api/app/items/${ITEMS}`, {
            ...A,
            body: { name: "Item Carol", order: carolOrderId },
        });
        const carolItemId = carolItem.created[0].id;
        await ok("POST", `/api/app/items/${NOTES}`, {
            ...A,
            body: { name: "Note Carol", item: carolItemId },
        });

        const hit = await ok("GET", `/api/app/items/${NOTES}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [
                        {
                            item: {
                                order: {
                                    customer: { full_name: { _eq: "Alice" } },
                                },
                            },
                        },
                    ],
                }),
            },
        });
        assert.strictEqual(hit.total, 1, "three-hop filter matches only Alice's note");
        assert.strictEqual(hit.data[0].name, "Note Alice");

        const miss = await ok("GET", `/api/app/items/${NOTES}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [
                        {
                            item: {
                                order: {
                                    customer: { full_name: { _eq: "Bob" } },
                                },
                            },
                        },
                    ],
                }),
            },
        });
        assert.strictEqual(miss.total, 0, "three-hop filter misses for Bob");
    });

    it("filters across a cross-app M:1 hop", async () => {
        const post = await ok("POST", `/api/app/items/${POSTS}`, {
            ...B,
            body: { title: "Cross Hop Post" },
        });
        const postId = post.created[0].id;
        const article = await ok("POST", `/api/app/items/${ARTICLES}`, {
            ...A,
            body: { headline: "Cross Hop Article", post: postId },
        });
        const articleId = article.created[0].id;
        await ok("POST", `/api/app/items/${COMMENTS}`, {
            ...A,
            body: { body: "Cross Hop Comment", article: articleId },
        });

        const data = await ok("GET", `/api/app/items/${COMMENTS}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [
                        { article: { post: { title: { _eq: "Cross Hop Post" } } } },
                    ],
                }),
            },
        });
        assert.strictEqual(data.total, 1, "cross-app multi-hop filter matches");
        assert.strictEqual(data.data[0].body, "Cross Hop Comment");

        const miss = await ok("GET", `/api/app/items/${COMMENTS}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [
                        { article: { post: { title: { _eq: "Does Not Exist" } } } },
                    ],
                }),
            },
        });
        assert.strictEqual(miss.total, 0, "cross-app multi-hop filter misses");
    });

    it("round-trips related_app on a relational section", async () => {
        const layout = await ok("POST", `/api/app/collections/${ARTICLES}/layouts`, {
            ...A,
            body: { name: `Layout ${SUFFIX}` },
        });
        const layoutId = layout.id;
        assert.ok(layoutId, "layout id returned");

        await ok(
            "POST",
            `/api/app/collections/${ARTICLES}/layouts/${layoutId}/sections`,
            {
                ...A,
                body: {
                    name: "Related comments",
                    section_type: "relational",
                    relation_field: "comments.article",
                    related_app: APP_B,
                    view_type: "table",
                    item_limit: 25,
                },
            },
        );

        const data = await ok(
            "GET",
            `/api/app/collections/${ARTICLES}/layouts/${layoutId}/sections`,
            { ...A },
        );
        assert.ok(Array.isArray(data.sections), "sections is an array");
        const section = data.sections.find((s: any) => s.name === "Related comments");
        assert.ok(section, "created section is listed");
        assert.strictEqual(section.related_app, APP_B, "related_app round-trips");
        assert.strictEqual(section.section_type, "relational");
        assert.strictEqual(section.relation_field, "comments.article");
        assert.strictEqual(section.item_limit, 25);
    });

    it("expands a three-hop M:1 relation via dotted fields", async () => {
        const fullName = `Expand ${SUFFIX}`;
        const customer = await ok("POST", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            body: { full_name: fullName },
        });
        const customerId = customer.created[0].id;
        const order = await ok("POST", `/api/app/items/${ORDERS}`, {
            ...A,
            body: { name: "Expand Order", customer: customerId },
        });
        const orderId = order.created[0].id;
        const item = await ok("POST", `/api/app/items/${ITEMS}`, {
            ...A,
            body: { name: "Expand Item", order: orderId },
        });
        const itemId = item.created[0].id;
        await ok("POST", `/api/app/items/${NOTES}`, {
            ...A,
            body: { name: "Expand Note", item: itemId },
        });

        const data = await ok("GET", `/api/app/items/${NOTES}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [
                        {
                            item: {
                                order: {
                                    customer: { full_name: { _eq: fullName } },
                                },
                            },
                        },
                    ],
                }),
                fields: JSON.stringify(["*", "item.order.customer.*"]),
            },
        });

        assert.strictEqual(data.total, 1, "one note matches the nested filter");
        const row = data.data[0];
        assert.strictEqual(typeof row.item, "object", "item is expanded");
        assert.strictEqual(typeof row.item.order, "object", "order is expanded");
        assert.strictEqual(
            typeof row.item.order.customer,
            "object",
            "customer is expanded (third hop)",
        );
        assert.strictEqual(row.item.id, itemId);
        assert.strictEqual(row.item.order.id, orderId);
        assert.strictEqual(row.item.order.customer.id, customerId);
        assert.strictEqual(
            row.item.order.customer.full_name,
            fullName,
            "deepest hop carries the parent value",
        );
    });

    it("filters across a three-hop M:1 chain that crosses apps", async () => {
        const customerName = `X3 Customer ${SUFFIX}`;
        const customer = await ok("POST", `/api/app/items/${B_CUSTOMERS}`, {
            ...B,
            body: { name: customerName },
        });
        const customerId = customer.created[0].id;
        const order = await ok("POST", `/api/app/items/${A_ORDERS}`, {
            ...A,
            body: { name: "X3 Order", customer: customerId },
        });
        const orderId = order.created[0].id;
        const item = await ok("POST", `/api/app/items/${A_ITEMS}`, {
            ...A,
            body: { name: "X3 Item", order: orderId },
        });
        const itemId = item.created[0].id;
        await ok("POST", `/api/app/items/${A_NOTES}`, {
            ...A,
            body: { name: "X3 Note", item: itemId },
        });

        const hit = await ok("GET", `/api/app/items/${A_NOTES}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [
                        {
                            item: {
                                order: {
                                    customer: { name: { _eq: customerName } },
                                },
                            },
                        },
                    ],
                }),
            },
        });
        assert.strictEqual(
            hit.total,
            1,
            `three-hop cross-app filter should match 1, got ${JSON.stringify(hit)}`,
        );
        assert.strictEqual(hit.data[0].name, "X3 Note");

        const miss = await ok("GET", `/api/app/items/${A_NOTES}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [
                        {
                            item: {
                                order: {
                                    customer: {
                                        name: { _eq: "No Such Customer" },
                                    },
                                },
                            },
                        },
                    ],
                }),
            },
        });
        assert.strictEqual(miss.total, 0, "three-hop cross-app filter misses");
    });

    it("retargets a relationship to a different app via PUT", async () => {
        // Link a record against app B's targets and prove B expansion.
        const bTarget = await ok("POST", `/api/app/items/${TARGETS}`, {
            ...B,
            body: { label: "B Target" },
        });
        const bTargetId = bTarget.created[0].id;
        await ok("POST", `/api/app/items/${REFS}`, {
            ...A,
            body: { name: "Ref B", target: bTargetId },
        });

        let collection = await ok("GET", `/api/app/collections/${REFS}`, { ...A });
        let targetField = collection.fields.find((f: any) => f.name === "target");
        assert.strictEqual(
            targetField.related_app,
            APP_B,
            "target initially points at app B",
        );

        const before = await ok("GET", `/api/app/items/${REFS}`, {
            ...A,
            query: {
                filter: JSON.stringify({ _and: [{ name: { _eq: "Ref B" } }] }),
                fields: JSON.stringify(["*", "target.*"]),
            },
        });
        assert.strictEqual(before.total, 1);
        assert.strictEqual(typeof before.data[0].target, "object");
        assert.strictEqual(before.data[0].target.label, "B Target");

        // Retarget the M:1 to app C's targets (same related_collection name).
        const put = await ok("PUT", `/api/app/collections/${REFS}`, {
            ...A,
            body: {
                fields: [
                    {
                        name: "target",
                        type: "relationship",
                        relationship_type: "many_to_one",
                        related_collection: TARGETS,
                        related_app: APP_C,
                        ordinal_position: 2,
                    },
                ],
                removed_fields: [],
            },
        });
        const putField = put.fields.find((f: any) => f.name === "target");
        assert.strictEqual(
            putField.related_app,
            APP_C,
            "PUT response reports the new related_app",
        );

        collection = await ok("GET", `/api/app/collections/${REFS}`, { ...A });
        targetField = collection.fields.find((f: any) => f.name === "target");
        assert.strictEqual(
            targetField.related_app,
            APP_C,
            "collection metadata reports the retargeted related_app",
        );

        // Retargeting drops + recreates the FK column, so links that pointed at
        // the old app are lost (the column value resets to NULL). This is the
        // current backend behavior, asserted here so a future fix is noticed.
        const stale = await ok("GET", `/api/app/items/${REFS}`, {
            ...A,
            query: {
                filter: JSON.stringify({ _and: [{ name: { _eq: "Ref B" } }] }),
                fields: JSON.stringify(["*", "target.*"]),
            },
        });
        assert.strictEqual(
            stale.data[0].target,
            null,
            "retarget drops the old app link (documented limitation)",
        );

        // A record created against app C now expands from C.
        const cTarget = await ok("POST", `/api/app/items/${TARGETS}`, {
            ...C,
            body: { label: "C Target" },
        });
        const cTargetId = cTarget.created[0].id;
        await ok("POST", `/api/app/items/${REFS}`, {
            ...A,
            body: { name: "Ref C", target: cTargetId },
        });

        const after = await ok("GET", `/api/app/items/${REFS}`, {
            ...A,
            query: {
                filter: JSON.stringify({ _and: [{ name: { _eq: "Ref C" } }] }),
                fields: JSON.stringify(["*", "target.*"]),
            },
        });
        assert.strictEqual(after.total, 1);
        assert.strictEqual(typeof after.data[0].target, "object");
        assert.strictEqual(
            after.data[0].target.label,
            "C Target",
            "expansion now resolves from app C",
        );
    });

    it("accepts a bare field filter map as an implicit _and", async () => {
        // The admin UI's relational sections send e.g.
        // `filter={"customer":{"_eq":"..."}}` with no `_and` wrapper.
        const created = await ok("POST", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            body: { full_name: "Bare Filter" },
        });
        const customerId = created.created[0].id;

        const bare = await ok("GET", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            query: { filter: JSON.stringify({ full_name: { _eq: "Bare Filter" } }) },
        });
        assert.strictEqual(bare.total, 1, "bare filter matches exactly one row");
        assert.strictEqual(bare.data[0].id, customerId);

        // Bare nested relation filter (no `_and`) must also work.
        await ok("POST", `/api/app/items/${ORDERS}`, {
            ...A,
            body: { name: "Bare Order", customer: customerId },
        });
        const nested = await ok("GET", `/api/app/items/${ORDERS}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    customer: { full_name: { _eq: "Bare Filter" } },
                }),
            },
        });
        assert.strictEqual(nested.total, 1, "bare nested filter matches");
    });

    it("filters parents by a same-app 1:M child without duplicating", async () => {
        const p1 = (
            await ok("POST", `/api/app/items/${CUSTOMERS}`, {
                ...A,
                body: { full_name: `P1 ${SUFFIX}` },
            })
        ).created[0].id;
        const p2 = (
            await ok("POST", `/api/app/items/${CUSTOMERS}`, {
                ...A,
                body: { full_name: `P2 ${SUFFIX}` },
            })
        ).created[0].id;
        // p1 has two matching orders, p2 has none.
        await ok("POST", `/api/app/items/${ORDERS}`, {
            ...A,
            body: { name: `Match A ${SUFFIX}`, customer: p1 },
        });
        await ok("POST", `/api/app/items/${ORDERS}`, {
            ...A,
            body: { name: `Match B ${SUFFIX}`, customer: p1 },
        });
        await ok("POST", `/api/app/items/${ORDERS}`, {
            ...A,
            body: { name: `Other ${SUFFIX}`, customer: p2 },
        });

        const data = await ok("GET", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [{ orders: { name: { _starts_with: "Match" } } }],
                }),
            },
        });
        assert.strictEqual(data.total, 1, "only p1 matches");
        assert.strictEqual(data.data.length, 1, "parent rows are not duplicated");
        assert.strictEqual(data.data[0].full_name, `P1 ${SUFFIX}`);
    });

    it("filters parents by a cross-app 1:M child", async () => {
        const c1 = (
            await ok("POST", `/api/app/items/${B_CUSTOMERS}`, {
                ...B,
                body: { name: `XC ${SUFFIX}` },
            })
        ).created[0].id;
        const c2 = (
            await ok("POST", `/api/app/items/${B_CUSTOMERS}`, {
                ...B,
                body: { name: `XC2 ${SUFFIX}` },
            })
        ).created[0].id;
        await ok("POST", `/api/app/items/${A_ORDERS}`, {
            ...A,
            body: { name: `XCOrder ${SUFFIX}`, customer: c1 },
        });
        await ok("POST", `/api/app/items/${A_ORDERS}`, {
            ...A,
            body: { name: `XCOther ${SUFFIX}`, customer: c2 },
        });

        const data = await ok("GET", `/api/app/items/${B_CUSTOMERS}`, {
            ...B,
            query: {
                filter: JSON.stringify({
                    _and: [{ orders: { name: { _eq: `XCOrder ${SUFFIX}` } } }],
                }),
            },
        });
        assert.strictEqual(data.total, 1, "cross-app 1:M filter matches one parent");
        assert.strictEqual(data.data[0].name, `XC ${SUFFIX}`);
    });

    it("filters through a 1:M then 1:M hop", async () => {
        const p = (
            await ok("POST", `/api/app/items/${CUSTOMERS}`, {
                ...A,
                body: { full_name: `Deep ${SUFFIX}` },
            })
        ).created[0].id;
        const o = (
            await ok("POST", `/api/app/items/${ORDERS}`, {
                ...A,
                body: { name: `DeepOrder ${SUFFIX}`, customer: p },
            })
        ).created[0].id;
        await ok("POST", `/api/app/items/${ITEMS}`, {
            ...A,
            body: { name: `DeepItem ${SUFFIX}`, order: o },
        });

        const data = await ok("GET", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [
                        { orders: { items: { name: { _eq: `DeepItem ${SUFFIX}` } } } },
                    ],
                }),
            },
        });
        assert.strictEqual(data.total, 1, "two 1:M hops resolve");
        assert.strictEqual(data.data[0].full_name, `Deep ${SUFFIX}`);
    });

    it("supports _nnull on a 1:M child field", async () => {
        const data = await ok("GET", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [{ orders: { name: { _nnull: null } } }],
                }),
            },
        });
        assert.ok(data.total >= 1, "customers with a named order match");
    });

    it("rejects dotted filter keys (nested only)", async () => {
        const res = await call("GET", `/api/app/items/${CUSTOMERS}`, {
            ...A,
            query: {
                filter: JSON.stringify({
                    _and: [{ "orders.name": { _eq: "x" } }],
                }),
            },
        });
        assert.strictEqual(res.status, 400, "dotted keys are rejected");
    });
});
