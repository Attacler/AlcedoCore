import { describe, it, expect, vi } from "vitest";
import { createKvResource } from "./kv.js";
import { createDbResource } from "./db.js";
import { createSchemaResource } from "./schema.js";
import { createLogsResource } from "./logs.js";
import { createDevResource } from "./dev.js";
import { createHealthResource } from "./health.js";
import { createPluginsResource } from "./plugins.js";
import { createMigrationsResource } from "./migrations.js";
import { createSettingsResource } from "./settings.js";
import { createUsageResource } from "./usage.js";
import { createClient } from "./client.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Creates a mock ky instance that records the last call for assertions. */
function mockKy(response: any = {}) {
    const jsonFn = vi.fn().mockResolvedValue(response);
    const thenable = { json: jsonFn, then: undefined };
    return {
        get: vi.fn((_url?: string, _opts?: any) => thenable),
        post: vi.fn((_url?: string, _opts?: any) => thenable),
        put: vi.fn((_url?: string, _opts?: any) => thenable),
        patch: vi.fn((_url?: string, _opts?: any) => thenable),
        delete: vi.fn((_url?: string, _opts?: any) => thenable),
        extend: vi.fn(() => mockKy(response).create()),
        create: vi.fn(() => mockKy(response)),
        _jsonFn: jsonFn,
    };
}

function getLastUrl(ky: ReturnType<typeof mockKy>): string {
    const methods = ["get", "post", "put", "patch", "delete"] as const;
    for (const m of methods) {
        if (ky[m].mock.calls.length > 0) {
            return ky[m].mock.calls[0][0] ?? "";
        }
    }
    return "";
}

function getLastBody(ky: ReturnType<typeof mockKy>): any {
    if (ky.post.mock.calls.length > 0) return ky.post.mock.calls[0][1]?.json;
    if (ky.put.mock.calls.length > 0) return ky.put.mock.calls[0][1]?.json;
    if (ky.patch.mock.calls.length > 0) return ky.patch.mock.calls[0][1]?.json;
    if (ky.delete.mock.calls.length > 0)
        return ky.delete.mock.calls[0][1]?.json;
    return undefined;
}

function getLastSearchParams(ky: ReturnType<typeof mockKy>): any {
    const methods = ["get", "post", "put", "patch", "delete"] as const;
    for (const m of methods) {
        if (ky[m].mock.calls.length > 0) {
            return ky[m].mock.calls[0][1]?.searchParams;
        }
    }
    return undefined;
}
// ---------------------------------------------------------------------------
// KV Resource
// ---------------------------------------------------------------------------

describe("KV Resource", () => {
    const response = { key: "test", value: "val" };

    it("get constructs correct URL and returns json", async () => {
        const ky = mockKy(response);
        const kv = createKvResource(ky);
        const result = await kv.get("my-key");
        expect(getLastUrl(ky)).toBe("kv/my-key");
        expect(result).toEqual(response);
    });

    it("set sends value in JSON body", async () => {
        const ky = mockKy({ success: true });
        const kv = createKvResource(ky);
        await kv.set("my-key", "hello", 3600);
        expect(getLastUrl(ky)).toBe("kv/my-key");
        expect(getLastBody(ky)?.value).toBe("hello");
        expect(getLastSearchParams(ky)).toEqual({ ttl: "3600" });
    });

    it("set serialises object values as JSON string", async () => {
        const ky = mockKy({ success: true });
        const kv = createKvResource(ky);
        await kv.set("my-key", { nested: true });
        expect(getLastBody(ky)?.value).toBe('{"nested":true}');
    });

    it("delete constructs correct URL", async () => {
        const ky = mockKy({ success: true });
        const kv = createKvResource(ky);
        await kv.delete("my-key");
        expect(getLastUrl(ky)).toBe("kv/my-key");
        expect(ky.delete).toHaveBeenCalled();
    });

    it("exists constructs correct URL", async () => {
        const ky = mockKy({ exists: true });
        const kv = createKvResource(ky);
        await kv.exists("my-key");
        expect(getLastUrl(ky)).toBe("kv/my-key/exists");
    });

    it("ttl constructs correct URL", async () => {
        const ky = mockKy({ ttl: 300 });
        const kv = createKvResource(ky);
        await kv.ttl("my-key");
        expect(getLastUrl(ky)).toBe("kv/my-key/ttl");
    });

    it("list with prefix sends query param", async () => {
        const ky = mockKy([{ key: "a" }]);
        const kv = createKvResource(ky);
        await kv.list("prefix-");
        expect(getLastUrl(ky)).toBe("kv");
        expect(getLastSearchParams(ky)).toEqual({ prefix: "prefix-" });
    });

    it("list without prefix omits query param", async () => {
        const ky = mockKy([]);
        const kv = createKvResource(ky);
        await kv.list();
        expect(getLastUrl(ky)).toBe("kv");
        expect(getLastSearchParams(ky)).toBeUndefined();
    });

    it("batch_get sends keys in body", async () => {
        const ky = mockKy({ items: [] });
        const kv = createKvResource(ky);
        await kv.batch_get(["a", "b"]);
        expect(getLastUrl(ky)).toBe("kv/batch/get");
        expect(getLastBody(ky)).toEqual({ keys: ["a", "b"] });
    });

    it("batch_set sends pairs in body", async () => {
        const ky = mockKy({ success: true });
        const kv = createKvResource(ky);
        await kv.batch_set([{ key: "a", value: 1 }]);
        expect(getLastUrl(ky)).toBe("kv/batch/set");
        expect(getLastBody(ky)).toEqual([{ key: "a", value: 1 }]);
    });

    it("batch_delete sends keys in body", async () => {
        const ky = mockKy({ success: true });
        const kv = createKvResource(ky);
        await kv.batch_delete(["a", "b"]);
        expect(getLastUrl(ky)).toBe("kv/batch/delete");
        expect(getLastBody(ky)).toEqual({ keys: ["a", "b"] });
    });

    it("query with pattern sends query param", async () => {
        const ky = mockKy([{ key: "a" }]);
        const kv = createKvResource(ky);
        await kv.query("a*");
        expect(getLastUrl(ky)).toBe("kv/query");
        expect(getLastSearchParams(ky)).toEqual({ pattern: "a*" });
    });

    it("query without pattern omits query param", async () => {
        const ky = mockKy([]);
        const kv = createKvResource(ky);
        await kv.query();
        expect(getLastUrl(ky)).toBe("kv/query");
        expect(getLastSearchParams(ky)).toBeUndefined();
    });
});

// ---------------------------------------------------------------------------
// DB Resource
// ---------------------------------------------------------------------------

describe("DB Resource", () => {
    it("query sends SQL with params and returns json", async () => {
        const ky = mockKy({ rows: [] });
        const db = createDbResource(ky);
        const result = await db.query(
            "my-plugin",
            "SELECT * FROM t",
            [1],
            60,
            500,
        );
        expect(getLastUrl(ky)).toBe("p/my-plugin/db/query");
        expect(getLastBody(ky)).toEqual({
            query: "SELECT * FROM t",
            params: [1],
            timeout_secs: 60,
            max_rows: 500,
        });
        expect(result).toEqual({ rows: [] });
    });

    it("query uses defaults for optional params", async () => {
        const ky = mockKy({ rows: [] });
        const db = createDbResource(ky);
        await db.query("my-plugin", "SELECT 1");
        expect(getLastBody(ky)).toEqual({
            query: "SELECT 1",
            params: [],
            timeout_secs: 30,
            max_rows: 100,
        });
    });
});

// ---------------------------------------------------------------------------
// Schema Resource
// ---------------------------------------------------------------------------

describe("Schema Resource", () => {
    it("get constructs correct URL", async () => {
        const ky = mockKy({ tables: [] });
        const schema = createSchemaResource(ky);
        const result = await schema.get("my-plugin");
        expect(getLastUrl(ky)).toBe("plugins/my-plugin/schema");
        expect(result).toEqual({ tables: [] });
    });
});

// ---------------------------------------------------------------------------
// Logs Resource
// ---------------------------------------------------------------------------

describe("Logs Resource", () => {
    it("list constructs correct URL", async () => {
        const ky = mockKy({ logs: [] });
        const logs = createLogsResource(ky);
        const result = await logs.list("my-plugin");
        expect(getLastUrl(ky)).toBe("plugins/my-plugin/logs");
        expect(result).toEqual({ logs: [] });
    });
});

// ---------------------------------------------------------------------------
// Dev Resource
// ---------------------------------------------------------------------------

describe("Dev Resource", () => {
    it("start sends slug, url, ttl", async () => {
        const ky = mockKy({ status: "started" });
        const dev = createDevResource(ky);
        const result = await dev.start(
            "my-plugin",
            "http://localhost:3000",
            7200,
        );
        expect(getLastUrl(ky)).toBe("dev/start");
        expect(getLastBody(ky)).toEqual({
            slug: "my-plugin",
            url: "http://localhost:3000",
            ttl_secs: 7200,
        });
        expect(result).toEqual({ status: "started" });
    });

    it("start uses default ttl when not provided", async () => {
        const ky = mockKy({ status: "started" });
        const dev = createDevResource(ky);
        await dev.start("my-plugin", "http://localhost:3000");
        expect(getLastBody(ky)?.ttl_secs).toBe(3600);
    });

    it("start rejects invalid URL schemes", async () => {
        const ky = mockKy({ status: "started" });
        const dev = createDevResource(ky);
        await expect(dev.start("my-plugin", "ftp://bad")).rejects.toThrow(
            "Invalid URL scheme",
        );
    });

    it("stop sends slug in body", async () => {
        const ky = mockKy({ status: "stopped" });
        const dev = createDevResource(ky);
        const result = await dev.stop("my-plugin");
        expect(getLastUrl(ky)).toBe("dev/stop");
        expect(getLastBody(ky)).toEqual({ slug: "my-plugin" });
        expect(result).toEqual({ status: "stopped" });
    });
});

// ---------------------------------------------------------------------------
// Health Resource
// ---------------------------------------------------------------------------

describe("Health Resource", () => {
    it("is a function that calls GET health", async () => {
        const ky = mockKy({ status: "ok", version: "1.0" });
        const health = createHealthResource(ky);
        const result = await health();
        expect(getLastUrl(ky)).toBe("health");
        expect(result).toEqual({ status: "ok", version: "1.0" });
    });
});

// ---------------------------------------------------------------------------
// Plugins Resource
// ---------------------------------------------------------------------------

describe("Plugins Resource", () => {
    it("list calls GET plugins", async () => {
        const ky = mockKy([{ name: "p1" }]);
        const plugins = createPluginsResource(ky);
        const result = await plugins.list();
        expect(getLastUrl(ky)).toBe("plugins");
        expect(result).toEqual([{ name: "p1" }]);
    });

    it("get calls GET plugins/:name", async () => {
        const ky = mockKy({ name: "p1" });
        const plugins = createPluginsResource(ky);
        const result = await plugins.get("p1");
        expect(getLastUrl(ky)).toBe("plugins/p1");
        expect(result).toEqual({ name: "p1" });
    });

    it("schema calls GET plugins/:name/schema", async () => {
        const ky = mockKy({ tables: [] });
        const plugins = createPluginsResource(ky);
        const result = await plugins.schema("p1");
        expect(getLastUrl(ky)).toBe("plugins/p1/schema");
        expect(result).toEqual({ tables: [] });
    });

    it("declarations calls GET plugins/:name/declarations", async () => {
        const ky = mockKy({ decls: [] });
        const plugins = createPluginsResource(ky);
        const result = await plugins.declarations("p1");
        expect(getLastUrl(ky)).toBe("plugins/p1/declarations");
        expect(result).toEqual({ decls: [] });
    });

    it("pages calls GET plugins/:name/pages and returns parsed pages", async () => {
        const ky = mockKy({ pages: [{ path: "/index", label: "Home" }] });
        const plugins = createPluginsResource(ky);
        const result = await plugins.pages("p1");
        expect(getLastUrl(ky)).toBe("plugins/p1/pages");
        expect(result).toEqual([{ path: "/index", label: "Home" }]);
    });

    it("assets calls GET plugins/:name/pages/assets", async () => {
        const ky = mockKy({ js: "app.js", css: "style.css" });
        const plugins = createPluginsResource(ky);
        const result = await plugins.assets("p1");
        expect(getLastUrl(ky)).toBe("plugins/p1/pages/assets");
        expect(result).toEqual({ js: "app.js", css: "style.css" });
    });

    it("install sends FormData with zip", async () => {
        const ky = mockKy({ name: "p1" });
        const plugins = createPluginsResource(ky);
        const zip = new Blob(["fake-zip"]);
        const result = await plugins.install(zip);
        expect(getLastUrl(ky)).toBe("plugins/install");
        // ky.post should have been called with a body that is FormData
        expect(ky.post.mock.calls[0][1]?.body).toBeInstanceOf(FormData);
        expect(result).toEqual({ name: "p1" });
    });

    it("uninstall calls DELETE plugins/uninstall with name", async () => {
        const ky = mockKy({ success: true });
        const plugins = createPluginsResource(ky);
        await plugins.uninstall("p1");
        expect(getLastUrl(ky)).toBe("plugins/uninstall");
        expect(getLastBody(ky)).toEqual({ name: "p1" });
    });

    it("update sends FormData with zip", async () => {
        const ky = mockKy({ name: "p1" });
        const plugins = createPluginsResource(ky);
        const zip = new Blob(["fake-zip"]);
        const result = await plugins.update("p1", zip);
        expect(getLastUrl(ky)).toBe("plugins/p1");
        expect(ky.put.mock.calls[0][1]?.body).toBeInstanceOf(FormData);
        expect(result).toEqual({ name: "p1" });
    });

    it("enable calls POST plugins/enable", async () => {
        const ky = mockKy({ success: true });
        const plugins = createPluginsResource(ky);
        await plugins.enable("p1");
        expect(getLastUrl(ky)).toBe("plugins/enable");
        expect(getLastBody(ky)).toEqual({ name: "p1" });
    });

    it("disable calls POST plugins/disable", async () => {
        const ky = mockKy({ success: true });
        const plugins = createPluginsResource(ky);
        await plugins.disable("p1");
        expect(getLastUrl(ky)).toBe("plugins/disable");
        expect(getLastBody(ky)).toEqual({ name: "p1" });
    });
});

// ---------------------------------------------------------------------------
// Migrations Resource
// ---------------------------------------------------------------------------

describe("Migrations Resource", () => {
    it("list calls GET plugins/:name/migrations", async () => {
        const ky = mockKy([{ name: "m1", pending: true }]);
        const migrations = createMigrationsResource(ky);
        const result = await migrations.list("my-plugin");
        expect(getLastUrl(ky)).toBe("plugins/my-plugin/migrations");
        expect(result).toEqual([{ name: "m1", pending: true }]);
    });

    it("run calls POST and resolves to void", async () => {
        const ky = mockKy({ success: true });
        const migrations = createMigrationsResource(ky);
        const result = await migrations.run("my-plugin");
        expect(getLastUrl(ky)).toBe("plugins/my-plugin/migrations");
        expect(result).toBeUndefined();
    });

    it("rollback calls POST with version", async () => {
        const ky = mockKy({ success: true });
        const migrations = createMigrationsResource(ky);
        const result = await migrations.rollback("my-plugin", "v1");
        expect(getLastUrl(ky)).toBe("plugins/my-plugin/rollback/v1");
        expect(result).toEqual({ success: true });
    });
});

// ---------------------------------------------------------------------------
// Settings Resource
// ---------------------------------------------------------------------------

describe("Settings Resource", () => {
    it("get calls GET plugins/:name/settings", async () => {
        const ky = mockKy({ theme: "dark" });
        const settings = createSettingsResource(ky);
        const result = await settings.get("my-plugin");
        expect(getLastUrl(ky)).toBe("plugins/my-plugin/settings");
        expect(result).toEqual({ theme: "dark" });
    });

    it("update calls PATCH with settings", async () => {
        const ky = mockKy({ success: true });
        const settings = createSettingsResource(ky);
        const newSettings = { theme: "light" };
        const result = await settings.update("my-plugin", newSettings);
        expect(getLastUrl(ky)).toBe("plugins/my-plugin/settings");
        expect(getLastBody(ky)).toEqual(newSettings);
        expect(result).toEqual({ success: true });
    });
});

// ---------------------------------------------------------------------------
// Usage Resource
// ---------------------------------------------------------------------------

describe("Usage Resource", () => {
    it("is a function that calls GET usage/:name", async () => {
        const ky = mockKy({ calls: 42 });
        const usage = createUsageResource(ky);
        const result = await usage("my-plugin");
        expect(getLastUrl(ky)).toBe("usage/my-plugin");
        expect(result).toEqual({ calls: 42 });
    });
});

// ---------------------------------------------------------------------------
// Client factory
// ---------------------------------------------------------------------------

describe("createClient", () => {
    it("returns an object with all resource keys", () => {
        const client = createClient("http://localhost:8080");
        expect(client).toHaveProperty("plugins");
        expect(client).toHaveProperty("health");
        expect(client).toHaveProperty("migrations");
        expect(client).toHaveProperty("settings");
        expect(client).toHaveProperty("usage");
        expect(client).toHaveProperty("kv");
        expect(client).toHaveProperty("db");
        expect(client).toHaveProperty("schema");
        expect(client).toHaveProperty("logs");
        expect(client).toHaveProperty("dev");
        expect(client).toHaveProperty("request");
    });

    it("accepts custom timeout and retry options", () => {
        const client = createClient("http://localhost:8080", {
            timeout: 5000,
            retry: { limit: 5 },
        });
        expect(client).toHaveProperty("kv");
    });
});
