import { describe, it, expect, vi } from "vitest";
import { resolveDownloadUrl, createFilesResource } from "./files.js";

describe("resolveDownloadUrl", () => {
    it("prefers the server download_url (cross-app safe)", () => {
        expect(
            resolveDownloadUrl("", undefined, {
                id: "f1",
                download_url:
                    "/api/app/files/f1/download?ac_app=crm&ac_version=production",
            }),
        ).toBe(
            "/api/app/files/f1/download?ac_app=crm&ac_version=production",
        );
    });

    it("builds the path from an id and explicit context", () => {
        expect(
            resolveDownloadUrl("http://host:8099/", undefined, "f1", {
                app: "crm",
                version: "prod",
            }),
        ).toBe(
            "http://host:8099/api/app/files/f1/download?ac_app=crm&ac_version=prod",
        );
    });

    it("falls back to the client defaults", () => {
        expect(
            resolveDownloadUrl("", { app: "a", version: "v" }, "f1"),
        ).toBe("/api/app/files/f1/download?ac_app=a&ac_version=v");
    });

    it("omits context when unknown", () => {
        expect(resolveDownloadUrl("", undefined, "f1")).toBe(
            "/api/app/files/f1/download",
        );
    });

    it("encodes the id", () => {
        expect(resolveDownloadUrl("", undefined, "a/b")).toBe(
            "/api/app/files/a%2Fb/download",
        );
    });
});

describe("files resource paths", () => {
    it("mounts under app/files", () => {
        const ky = {
            post: vi.fn(() => ({ json: vi.fn() })),
            get: vi.fn(() => ({ json: vi.fn() })),
        } as any;
        const files = createFilesResource(ky, "", undefined);
        files.upload(new Blob(), "x.txt");
        expect(ky.post.mock.calls[0][0]).toBe("app/files/upload");
    });
});
