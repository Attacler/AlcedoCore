import { describe, it, expect, afterEach } from "vitest";
import http from "node:http";
import { createClient, resolveAppHeaders } from "./client.js";

describe("resolveAppHeaders", () => {
    it("falls back to client defaults", () => {
        const headers = new Headers();
        resolveAppHeaders({ app: "shop", version: "production" }, undefined, headers);
        expect(headers.get("X-App")).toBe("shop");
        expect(headers.get("X-Version")).toBe("production");
    });
    it("per-call options win", () => {
        const headers = new Headers();
        resolveAppHeaders({ app: "shop", version: "production" }, { app: "billing", version: "v2" }, headers);
        expect(headers.get("X-App")).toBe("billing");
        expect(headers.get("X-Version")).toBe("v2");
    });
    it("omits headers when neither is set", () => {
        const headers = new Headers();
        resolveAppHeaders(undefined, undefined, headers);
        expect(headers.get("X-App")).toBeNull();
        expect(headers.get("X-Version")).toBeNull();
    });
});

describe("createClient apiKey", () => {
    it("sends the key as a bearer token without per-call wiring", async () => {
        const seen: string[] = [];
        const server = http.createServer((req, res) => {
            seen.push(req.headers.authorization ?? "");
            res.writeHead(200, { "Content-Type": "application/json" });
            res.end("{}");
        });
        await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
        const port = (server.address() as any).port;

        try {
            const client = createClient(`http://127.0.0.1:${port}`, {
                apiKey: "dev_test",
                app: "shop",
                version: "production",
            });
            await client.kv.get("some-key");
            await client.kv.list("pre");

            expect(seen).toHaveLength(2);
            for (const header of seen) {
                expect(header).toBe("Bearer dev_test");
            }
        } finally {
            await new Promise<void>((r) => server.close(() => r()));
        }
    });

    it("sends no Authorization header when unset", async () => {
        const seen: Array<string | undefined> = [];
        const server = http.createServer((req, res) => {
            seen.push(req.headers.authorization);
            res.writeHead(200, { "Content-Type": "application/json" });
            res.end("{}");
        });
        await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
        const port = (server.address() as any).port;

        try {
            const client = createClient(`http://127.0.0.1:${port}`);
            await client.kv.get("some-key");
            expect(seen).toEqual([undefined]);
        } finally {
            await new Promise<void>((r) => server.close(() => r()));
        }
    });
});

describe("createClient dev + db resources", () => {
    const OLD_ENV = process.env.PLUGIN_BASE_PATH;

    afterEach(() => {
        if (OLD_ENV === undefined) delete process.env.PLUGIN_BASE_PATH;
        else process.env.PLUGIN_BASE_PATH = OLD_ENV;
    });

    function startServer(handler: (req: http.IncomingMessage, res: http.ServerResponse) => void) {
        const server = http.createServer(handler);
        return new Promise<{ server: http.Server; port: number }>((resolve) =>
            server.listen(0, "127.0.0.1", () =>
                resolve({ server, port: (server.address() as any).port }),
            ),
        );
    }

    it("dev.requestId posts to /api/dev/request-id and passes the bare response through", async () => {
        const seen: Array<{ url?: string; method?: string; body: string }> = [];
        const { server, port } = await startServer((req, res) => {
            let body = "";
            req.on("data", (c) => (body += c));
            req.on("end", () => {
                seen.push({ url: req.url, method: req.method, body });
                res.writeHead(200, { "Content-Type": "application/json" });
                res.end(JSON.stringify({ request_id: "rid-1" }));
            });
        });

        try {
            const client = createClient(`http://127.0.0.1:${port}`);
            const result = await client.dev.requestId("my-plugin");
            expect(result).toEqual({ request_id: "rid-1" });
            expect(seen[0].method).toBe("POST");
            expect(seen[0].url).toBe("/api/dev/request-id");
            expect(JSON.parse(seen[0].body)).toEqual({ slug: "my-plugin" });
        } finally {
            await new Promise<void>((r) => server.close(() => r()));
        }
    });

    it("db.query resolves the proxy path from PLUGIN_BASE_PATH and unwraps JSend", async () => {
        process.env.PLUGIN_BASE_PATH = "/p/7";
        const seen: Array<{ url?: string; method?: string; body: string }> = [];
        const { server, port } = await startServer((req, res) => {
            let body = "";
            req.on("data", (c) => (body += c));
            req.on("end", () => {
                seen.push({ url: req.url, method: req.method, body });
                res.writeHead(200, { "Content-Type": "application/json" });
                res.end(
                    JSON.stringify({
                        status: "success",
                        data: { rows: [[1]], row_count: 1 },
                    }),
                );
            });
        });

        try {
            const client = createClient(`http://127.0.0.1:${port}`);
            const result = await client.db.query("SELECT 1", [], 30, 100);
            expect(seen[0].method).toBe("POST");
            expect(seen[0].url).toBe("/p/7/db/query");
            expect(JSON.parse(seen[0].body)).toEqual({
                query: "SELECT 1",
                params: [],
                timeout_secs: 30,
                max_rows: 100,
            });
            expect(result).toEqual({ rows: [[1]], row_count: 1 });
        } finally {
            await new Promise<void>((r) => server.close(() => r()));
        }
    });

    it("db.query prefers an explicit basePath over PLUGIN_BASE_PATH", async () => {
        process.env.PLUGIN_BASE_PATH = "/p/7";
        const seen: string[] = [];
        const { server, port } = await startServer((req, res) => {
            seen.push(req.url ?? "");
            res.writeHead(200, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ status: "success", data: { rows: [] } }));
        });

        try {
            const client = createClient(`http://127.0.0.1:${port}`);
            await client.db.query("SELECT 1", [], 30, 100, { basePath: "/p/9" });
            expect(seen[0]).toBe("/p/9/db/query");
        } finally {
            await new Promise<void>((r) => server.close(() => r()));
        }
    });
});
