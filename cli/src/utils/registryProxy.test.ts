import { describe, it, expect, afterEach } from "vitest";
import http from "node:http";
import { startRegistryForwarder, PROXY_PREFIX } from "./registryProxy";

describe("startRegistryForwarder", () => {
    const closers: Array<() => Promise<void>> = [];
    afterEach(async () => {
        for (const c of closers) await c();
        closers.length = 0;
    });

    it("forwards /v2 paths unchanged and injects the bearer key", async () => {
        const seen: { url?: string; auth?: string } = {};
        const core = http.createServer((req, res) => {
            seen.url = req.url;
            seen.auth = req.headers["authorization"] as string;
            res.writeHead(200);
            res.end("ok");
        });
        await new Promise<void>((r) => core.listen(0, "127.0.0.1", () => r()));
        const corePort = (core.address() as any).port;

        const fwd = await startRegistryForwarder({
            coreUrl: `http://127.0.0.1:${corePort}`,
            apiKey: "dev_test",
            port: 0,
        });
        closers.push(fwd.close, () => new Promise<void>((r) => core.close(() => r())));

        const res = await fetch(`http://127.0.0.1:${fwd.port}${PROXY_PREFIX}/foo/tags/list`);
        expect(res.status).toBe(200);
        // The core serves the registry API on /v2 itself, so the path is
        // forwarded verbatim rather than being re-prefixed.
        expect(seen.url).toBe(`${PROXY_PREFIX}/foo/tags/list`);
        expect(seen.auth).toBe("Bearer dev_test");
    });

    it("refuses to forward non-registry paths", async () => {
        let coreHits = 0;
        const core = http.createServer((_req, res) => {
            coreHits++;
            res.writeHead(200);
            res.end("ok");
        });
        await new Promise<void>((r) => core.listen(0, "127.0.0.1", () => r()));
        const corePort = (core.address() as any).port;

        const fwd = await startRegistryForwarder({
            coreUrl: `http://127.0.0.1:${corePort}`,
            apiKey: "dev_test",
            port: 0,
        });
        closers.push(fwd.close, () => new Promise<void>((r) => core.close(() => r())));

        // Without this guard a local caller could reach the core's admin API
        // with the developer key attached.
        const res = await fetch(`http://127.0.0.1:${fwd.port}/api/platform/users`);
        expect(res.status).toBe(404);
        expect(coreHits).toBe(0);
    });

    it("auto-selects a free port when none is given", async () => {
        const seen: { url?: string } = {};
        const core = http.createServer((req, res) => {
            seen.url = req.url;
            res.writeHead(200);
            res.end("ok");
        });
        await new Promise<void>((r) => core.listen(0, "127.0.0.1", () => r()));
        const corePort = (core.address() as any).port;

        const fwd = await startRegistryForwarder({
            coreUrl: `http://127.0.0.1:${corePort}`,
            apiKey: "dev_test",
        });
        closers.push(fwd.close, () => new Promise<void>((r) => core.close(() => r())));

        expect(typeof fwd.port).toBe("number");
        expect(fwd.port).toBeGreaterThan(0);

        const res = await fetch(`http://127.0.0.1:${fwd.port}${PROXY_PREFIX}/foo`);
        expect(res.status).toBe(200);
        expect(seen.url).toBe(`${PROXY_PREFIX}/foo`);
    });
});
