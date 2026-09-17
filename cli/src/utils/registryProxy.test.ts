import { describe, it, expect, afterEach } from "vitest";
import http from "node:http";
import { startRegistryForwarder, PROXY_PREFIX } from "./registryProxy";

describe("startRegistryForwarder", () => {
    const closers: Array<() => Promise<void>> = [];
    afterEach(async () => {
        for (const c of closers) await c();
        closers.length = 0;
    });

    it("prefixes /v2 paths and injects the bearer key", async () => {
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

        const res = await fetch(`http://127.0.0.1:${fwd.port}/v2/foo/tags/list`);
        expect(res.status).toBe(200);
        expect(seen.url).toBe(`${PROXY_PREFIX}/v2/foo/tags/list`);
        expect(seen.auth).toBe("Bearer dev_test");
    });

    it("passes already-prefixed paths through unchanged", async () => {
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
            port: 0,
        });
        closers.push(fwd.close, () => new Promise<void>((r) => core.close(() => r())));

        await fetch(`http://127.0.0.1:${fwd.port}${PROXY_PREFIX}/v2/foo`);
        expect(seen.url).toBe(`${PROXY_PREFIX}/v2/foo`);
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

        const res = await fetch(`http://127.0.0.1:${fwd.port}/v2/foo`);
        expect(res.status).toBe(200);
        expect(seen.url).toBe(`${PROXY_PREFIX}/v2/foo`);
    });
});
