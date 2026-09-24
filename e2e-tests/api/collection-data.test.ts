/**
 * corev2 collection-data API test.
 *
 * Consumes the HTTP API directly (no SDK) and proves:
 *  - collection CRUD (create / get / update / delete)
 *  - record CRUD (create / list / get / patch / delete)
 *  - pagination (`page`/`per_page` -> `limit`/`offset`/`total`)
 *  - filtering (`_and`, `_or`, `_eq`, `_null`)
 *  - sorting (`sort` as a JSON array)
 *  - expanded relationship objects (`fields=["*","author.*"]`)
 *  - recursive nested relational writes (Directus-style), unlimited depth
 *
 * Run (corev2 must be running):
 *   cd e2e-tests
 *   npx tsx --test ./api/collection-data.test.ts
 *
 * Env overrides: CORE_URL, TEST_APP, TEST_VERSION, ADMIN_EMAIL, ADMIN_PASSWORD.
 */
import { describe, it, before, after } from "node:test";
import assert from "node:assert";

const API = process.env.CORE_URL ?? "http://localhost:8080";
const APP = process.env.TEST_APP ?? "test";
const VERSION = process.env.TEST_VERSION ?? "production";
const EMAIL = process.env.ADMIN_EMAIL ?? "admin@alcedo.dev";
const PASSWORD = process.env.ADMIN_PASSWORD ?? "admin123!";

const SUFFIX = Math.random().toString(36).slice(2, 8);
const AUTHORS = `apitest_authors_${SUFFIX}`;
const CITIES = `apitest_cities_${SUFFIX}`;
const STREETS = `apitest_streets_${SUFFIX}`;
const HOUSES = `apitest_houses_${SUFFIX}`;
const ROOMS = `apitest_rooms_${SUFFIX}`;

let cookie = "";
const createdCollections: string[] = [];

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

function headers(c?: string): Record<string, string> {
    const h: Record<string, string> = {
        "Content-Type": "application/json",
        "X-App": APP,
        "X-Version": VERSION,
    };
    if (c) h["Cookie"] = c;
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
    if (res.status !== 200)
        throw new Error(`Login failed: ${res.status} ${text}`);
    const setCookie = res.headers.get("set-cookie") || "";
    const match = setCookie.match(/alcedo_session=([^;]+)/);
    if (!match) throw new Error(`No session cookie returned: ${setCookie}`);
    return `alcedo_session=${match[1]}`;
}

interface CallOptions {
    cookie?: string;
    body?: unknown;
    query?: Record<string, string>;
}

async function call(
    method: string,
    path: string,
    opts: CallOptions = {},
): Promise<{ status: number; json: any }> {
    let url = `${API}${path}`;
    if (opts.query && Object.keys(opts.query).length > 0) {
        const params = new URLSearchParams(opts.query);
        url += `?${params.toString()}`;
    }
    const res = await fetch(url, {
        method,
        headers: headers(opts.cookie),
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

/** Asserts a successful JSend response and returns its `data`. */
async function ok(
    method: string,
    path: string,
    opts: CallOptions = {},
): Promise<any> {
    const res = await call(method, path, { cookie, ...opts });
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

function listQuery(
    query: Record<string, string>,
): Record<string, string> {
    return query;
}

/** Fetches exactly one row matching `field = value`. */
async function findOne(
    collection: string,
    field: string,
    value: string,
): Promise<any> {
    const data = await ok("GET", `/api/app/items/${collection}`, {
        query: listQuery({
            filter: JSON.stringify({ _and: [{ [field]: { _eq: value } }] }),
        }),
    });
    assert.strictEqual(
        data.total,
        1,
        `expected 1 ${collection} where ${field}=${value}, got ${data.total}`,
    );
    return data.data[0];
}

// ---------------------------------------------------------------------------
// Setup / teardown
// ---------------------------------------------------------------------------

async function createCollection(
    name: string,
    fields: any[],
): Promise<void> {
    await ok("POST", "/api/app/collections", {
        body: { name, display_name: name, fields },
    });
    createdCollections.push(name);
}

async function deleteCollection(name: string): Promise<void> {
    await call("DELETE", `/api/app/collections/${name}`, { cookie });
}

before(async () => {
    cookie = await login();

    // authors <- cities <- streets (1:M at each level, M:1 the other way)
    await createCollection(AUTHORS, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
    ]);
    await createCollection(CITIES, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        {
            name: "author",
            type: "relationship",
            relationship_type: "many_to_one",
            related_collection: AUTHORS,
            ordinal_position: 2,
        },
    ]);
    await createCollection(STREETS, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        {
            name: "city",
            type: "relationship",
            relationship_type: "many_to_one",
            related_collection: CITIES,
            ordinal_position: 2,
        },
    ]);
    await createCollection(HOUSES, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        {
            name: "street",
            type: "relationship",
            relationship_type: "many_to_one",
            related_collection: STREETS,
            ordinal_position: 2,
        },
    ]);
    await createCollection(ROOMS, [
        { name: "name", type: "string", required: true, ordinal_position: 1 },
        {
            name: "house",
            type: "relationship",
            relationship_type: "many_to_one",
            related_collection: HOUSES,
            ordinal_position: 2,
        },
    ]);

    // Add the virtual 1:M sides.
    await ok("PUT", `/api/app/collections/${AUTHORS}`, {
        body: {
            fields: [
                {
                    name: "cities",
                    type: "relationship",
                    relationship_type: "one_to_many",
                    related_collection: CITIES,
                    ordinal_position: 2,
                },
            ],
            removed_fields: [],
        },
    });
    await ok("PUT", `/api/app/collections/${CITIES}`, {
        body: {
            fields: [
                {
                    name: "streets",
                    type: "relationship",
                    relationship_type: "one_to_many",
                    related_collection: STREETS,
                    ordinal_position: 3,
                },
            ],
            removed_fields: [],
        },
    });
    await ok("PUT", `/api/app/collections/${STREETS}`, {
        body: {
            fields: [
                {
                    name: "houses",
                    type: "relationship",
                    relationship_type: "one_to_many",
                    related_collection: HOUSES,
                    ordinal_position: 3,
                },
            ],
            removed_fields: [],
        },
    });
    await ok("PUT", `/api/app/collections/${HOUSES}`, {
        body: {
            fields: [
                {
                    name: "rooms",
                    type: "relationship",
                    relationship_type: "one_to_many",
                    related_collection: ROOMS,
                    ordinal_position: 3,
                },
            ],
            removed_fields: [],
        },
    });
});

after(async () => {
    // Delete children before parents to avoid FK restrictions.
    for (const name of [...createdCollections].reverse()) {
        try {
            await deleteCollection(name);
        } catch {
            /* best effort */
        }
    }
});

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("corev2 collection data API", () => {
    it("creates collections and returns their fields", async () => {
        const collection = await ok(
            "GET",
            `/api/app/collections/${AUTHORS}`,
        );
        assert.strictEqual(collection.name, AUTHORS);
        const names = collection.fields.map((f: any) => f.name);
        assert.ok(names.includes("name"), "has name field");
        assert.ok(names.includes("cities"), "has virtual 1:M field");
        const cities = collection.fields.find((f: any) => f.name === "cities");
        assert.strictEqual(cities.relationship_type, "one_to_many");
    });

    it("lists collections", async () => {
        const data = await ok("GET", "/api/app/collections");
        assert.ok(Array.isArray(data.collections));
        const names = data.collections.map((c: any) => c.name);
        assert.ok(names.includes(AUTHORS));
    });

    it("creates a record from a single object", async () => {
        const data = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "Acme" },
        });
        assert.ok(Array.isArray(data.created), "returns created[]");
        assert.strictEqual(data.created.length, 1);
        assert.ok(data.created[0].id, "created record has an id");
        assert.strictEqual(data.created[0].name, "Acme");
    });

    it("gets a record by id", async () => {
        const created = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "Get Me" },
        });
        const id = created.created[0].id;
        const item = await ok("GET", `/api/app/items/${AUTHORS}/${id}`);
        assert.strictEqual(item.id, id);
        assert.strictEqual(item.name, "Get Me");
    });

    it("updates a record via PATCH by id", async () => {
        const created = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "Before" },
        });
        const id = created.created[0].id;
        const updated = await ok("PATCH", `/api/app/items/${AUTHORS}/${id}`, {
            body: { name: "After" },
        });
        assert.strictEqual(updated.name, "After");
        const reread = await ok("GET", `/api/app/items/${AUTHORS}/${id}`);
        assert.strictEqual(reread.name, "After");
    });

    it("lists with pagination and reports a total", async () => {
        for (const name of ["P1", "P2", "P3"]) {
            await ok("POST", `/api/app/items/${AUTHORS}`, { body: { name } });
        }
        const page1 = await ok(
            "GET",
            `/api/app/items/${AUTHORS}`,
            {
                query: listQuery({ page: "1", per_page: "2" }),
            },
        );
        assert.strictEqual(page1.limit, 2);
        assert.strictEqual(page1.offset, 0);
        assert.strictEqual(page1.data.length, 2);
        assert.ok(page1.total >= 3, `total should be >= 3, got ${page1.total}`);

        const page2 = await ok("GET", `/api/app/items/${AUTHORS}`, {
            query: listQuery({ page: "2", per_page: "2" }),
        });
        assert.strictEqual(page2.offset, 2);
        assert.notStrictEqual(
            page2.data[0].id,
            page1.data[0].id,
            "page 2 differs from page 1",
        );
    });

    it("filters with _and/_eq", async () => {
        const data = await ok("GET", `/api/app/items/${AUTHORS}`, {
            query: listQuery({
                filter: JSON.stringify({
                    _and: [{ name: { _eq: "After" } }],
                }),
            }),
        });
        assert.strictEqual(data.total, 1);
        assert.strictEqual(data.data[0].name, "After");
    });

    it("filters with a top-level _or", async () => {
        const data = await ok("GET", `/api/app/items/${AUTHORS}`, {
            query: listQuery({
                filter: JSON.stringify({
                    _or: [
                        { name: { _eq: "Acme" } },
                        { name: { _eq: "After" } },
                    ],
                }),
            }),
        });
        assert.strictEqual(data.total, 2, "OR should match both rows");
    });

    it("sorts using a JSON array", async () => {
        const data = await ok("GET", `/api/app/items/${AUTHORS}`, {
            query: listQuery({ sort: JSON.stringify(["-name"]) }),
        });
        const names = data.data.map((i: any) => i.name);
        const sorted = [...names].sort().reverse();
        assert.deepStrictEqual(names, sorted, "rows sorted by name desc");
    });

    it("expands a M:1 relation into an object via fields", async () => {
        const author = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "Rel Author" },
        });
        const authorId = author.created[0].id;
        await ok("POST", `/api/app/items/${CITIES}`, {
            body: { name: "Rel City", author: authorId },
        });

        const data = await ok("GET", `/api/app/items/${CITIES}`, {
            query: listQuery({
                filter: JSON.stringify({
                    _and: [{ name: { _eq: "Rel City" } }],
                }),
                fields: JSON.stringify(["*", "author.*"]),
            }),
        });
        assert.strictEqual(data.data.length, 1);
        const city = data.data[0];
        assert.strictEqual(typeof city.author, "object", "author is expanded");
        assert.strictEqual(city.author.id, authorId);
        assert.strictEqual(city.author.name, "Rel Author");
    });

    it("creates a nested M:1 record on POST", async () => {
        const data = await ok("POST", `/api/app/items/${CITIES}`, {
            body: { name: "Nested City", author: { name: "Nested Author" } },
        });
        const city = data.created[0];
        assert.ok(city.author, "city got a FK");

        const author = await ok(
            "GET",
            `/api/app/items/${AUTHORS}/${city.author}`,
        );
        assert.strictEqual(author.name, "Nested Author");
    });

    it("updates a related M:1 record without changing the FK", async () => {
        const author = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "FK Author" },
        });
        const authorId = author.created[0].id;
        const city = await ok("POST", `/api/app/items/${CITIES}`, {
            body: { name: "FK City", author: authorId },
        });
        const cityId = city.created[0].id;

        const updated = await ok("PATCH", `/api/app/items/${CITIES}/${cityId}`, {
            body: { author: { id: authorId, name: "FK Author Updated" } },
        });
        assert.strictEqual(updated.author, authorId, "FK unchanged");

        const reread = await ok("GET", `/api/app/items/${AUTHORS}/${authorId}`);
        assert.strictEqual(reread.name, "FK Author Updated");
    });

    it("unlinks a M:1 relation with null", async () => {
        const author = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "Unlink Author" },
        });
        const authorId = author.created[0].id;
        const city = await ok("POST", `/api/app/items/${CITIES}`, {
            body: { name: "Unlink City", author: authorId },
        });
        const cityId = city.created[0].id;

        const updated = await ok("PATCH", `/api/app/items/${CITIES}/${cityId}`, {
            body: { author: null },
        });
        assert.strictEqual(updated.author, null);

        const data = await ok("GET", `/api/app/items/${CITIES}`, {
            query: listQuery({
                filter: JSON.stringify({
                    _and: [{ id: { _eq: cityId } }, { author: { _null: null } }],
                }),
            }),
        });
        assert.strictEqual(data.total, 1, "_null matches the unlinked city");
    });

    it("creates an unlimited-depth nested tree in one request", async () => {
        const data = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: {
                name: "Deep Author",
                cities: {
                    create: [
                        {
                            name: "Deep City",
                            streets: {
                                create: [{ name: "Deep Street" }],
                            },
                        },
                    ],
                },
            },
        });
        const authorId = data.created[0].id;

        // author -> city
        const cities = await ok("GET", `/api/app/items/${CITIES}`, {
            query: listQuery({
                filter: JSON.stringify({
                    _and: [{ author: { _eq: authorId } }],
                }),
            }),
        });
        assert.strictEqual(cities.total, 1);
        assert.strictEqual(cities.data[0].name, "Deep City");

        // city -> street (third level)
        const streets = await ok("GET", `/api/app/items/${STREETS}`, {
            query: listQuery({
                filter: JSON.stringify({
                    _and: [{ city: { _eq: cities.data[0].id } }],
                }),
            }),
        });
        assert.strictEqual(streets.total, 1);
        assert.strictEqual(streets.data[0].name, "Deep Street");
    });

    it("creates a 5-level deep nested tree in one request", async () => {
        const data = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: {
                name: "L5 Author",
                cities: {
                    create: [
                        {
                            name: "L5 City",
                            streets: {
                                create: [
                                    {
                                        name: "L5 Street",
                                        houses: {
                                            create: [
                                                {
                                                    name: "L5 House",
                                                    rooms: {
                                                        create: [
                                                            { name: "L5 Room" },
                                                        ],
                                                    },
                                                },
                                            ],
                                        },
                                    },
                                ],
                            },
                        },
                    ],
                },
            },
        });
        const authorId = data.created[0].id;

        const city = await findOne(CITIES, "author", authorId);
        const street = await findOne(STREETS, "city", city.id);
        const house = await findOne(HOUSES, "street", street.id);
        const room = await findOne(ROOMS, "house", house.id);

        assert.strictEqual(city.name, "L5 City");
        assert.strictEqual(street.name, "L5 Street");
        assert.strictEqual(house.name, "L5 House");
        assert.strictEqual(room.name, "L5 Room");
    });

    it("updates a deeply nested record in one PATCH", async () => {
        const data = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: {
                name: "L5U Author",
                cities: {
                    create: [
                        {
                            name: "L5U City",
                            streets: {
                                create: [
                                    {
                                        name: "L5U Street",
                                        houses: {
                                            create: [{ name: "L5U House" }],
                                        },
                                    },
                                ],
                            },
                        },
                    ],
                },
            },
        });
        const authorId = data.created[0].id;
        const city = await findOne(CITIES, "author", authorId);
        const street = await findOne(STREETS, "city", city.id);
        const house = await findOne(HOUSES, "street", street.id);

        await ok("PATCH", `/api/app/items/${AUTHORS}/${authorId}`, {
            body: {
                cities: {
                    update: [
                        {
                            id: city.id,
                            streets: {
                                update: [
                                    {
                                        id: street.id,
                                        name: "L5U Street Renamed",
                                        houses: {
                                            update: [
                                                {
                                                    id: house.id,
                                                    name: "L5U House Renamed",
                                                },
                                            ],
                                        },
                                    },
                                ],
                            },
                        },
                    ],
                },
            },
        });

        const streetAfter = await ok(
            "GET",
            `/api/app/items/${STREETS}/${street.id}`,
        );
        const houseAfter = await ok(
            "GET",
            `/api/app/items/${HOUSES}/${house.id}`,
        );
        assert.strictEqual(streetAfter.name, "L5U Street Renamed");
        assert.strictEqual(houseAfter.name, "L5U House Renamed");
    });

    it("handles O2M create/update/delete in one PATCH", async () => {
        const author = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "O2M Author" },
        });
        const authorId = author.created[0].id;

        // create two cities
        await ok("PATCH", `/api/app/items/${AUTHORS}/${authorId}`, {
            body: { cities: { create: [{ name: "O2M A" }, { name: "O2M B" }] } },
        });
        let cities = await ok("GET", `/api/app/items/${CITIES}`, {
            query: listQuery({
                filter: JSON.stringify({
                    _and: [{ author: { _eq: authorId } }],
                }),
            }),
        });
        assert.strictEqual(cities.total, 2);

        const a = cities.data.find((c: any) => c.name === "O2M A");
        const b = cities.data.find((c: any) => c.name === "O2M B");

        // update A, delete B
        await ok("PATCH", `/api/app/items/${AUTHORS}/${authorId}`, {
            body: {
                cities: {
                    update: [{ id: a.id, name: "O2M A Updated" }],
                    delete: [b.id],
                },
            },
        });

        const updatedA = await ok("GET", `/api/app/items/${CITIES}/${a.id}`);
        assert.strictEqual(updatedA.name, "O2M A Updated");

        cities = await ok("GET", `/api/app/items/${CITIES}`, {
            query: listQuery({
                filter: JSON.stringify({
                    _and: [{ author: { _eq: authorId } }],
                }),
            }),
        });
        assert.strictEqual(cities.total, 1, "one child deleted");
    });

    it("deletes records via a pk_values body", async () => {
        const created = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "Delete Me" },
        });
        const id = created.created[0].id;

        const deleted = await ok("DELETE", `/api/app/items/${AUTHORS}`, {
            body: { pk_values: [id] },
        });
        assert.strictEqual(deleted.deleted, 1);

        const res = await call("GET", `/api/app/items/${AUTHORS}/${id}`);
        assert.strictEqual(res.status, 404, "record is gone");
    });

    it("returns references shape", async () => {
        const created = await ok("POST", `/api/app/items/${AUTHORS}`, {
            body: { name: "Refs" },
        });
        const id = created.created[0].id;
        const data = await ok(
            "GET",
            `/api/app/items/${AUTHORS}/${id}/references`,
        );
        assert.ok(Array.isArray(data.references));
    });
});
