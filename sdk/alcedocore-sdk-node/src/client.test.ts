import { describe, it, expect } from "vitest";
import { resolveAppHeaders } from "./client.js";

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
