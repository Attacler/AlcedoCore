import { describe, it, expect } from "vitest";
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
