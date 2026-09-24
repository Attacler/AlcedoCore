import { describe, it, expect } from "vitest";
import {
    createRelationBody,
    addCreate,
    replaceCreate,
    removeCreate,
    upsertUpdate,
    removeUpdate,
    addDelete,
    isRelationBodyEmpty,
    serializeRelationBody,
    mergeRelationBodies,
    isTempId,
    type RelationBody,
} from "./useRelationBody";

function seeded(): RelationBody {
    return createRelationBody();
}

describe("create/update/delete bookkeeping", () => {
    it("adds creates and returns a temp id", () => {
        const body = seeded();
        const tempId = addCreate(body, { first_name: "Ava" });
        expect(isTempId(tempId)).toBe(true);
        expect(body.create).toHaveLength(1);
        expect(body.create[0].values).toEqual({ first_name: "Ava" });
    });

    it("replaces a queued create in place", () => {
        const body = seeded();
        const tempId = addCreate(body, { first_name: "Ava" });
        replaceCreate(body, tempId, { first_name: "Ava2" });
        expect(body.create).toHaveLength(1);
        expect(body.create[0].values).toEqual({ first_name: "Ava2" });
    });

    it("removes a queued create", () => {
        const body = seeded();
        const tempId = addCreate(body, { first_name: "Ava" });
        removeCreate(body, tempId);
        expect(body.create).toHaveLength(0);
        expect(isRelationBodyEmpty(body)).toBe(true);
    });

    it("upserts and merges updates for the same id", () => {
        const body = seeded();
        upsertUpdate(body, "r1", { first_name: "Ava" });
        upsertUpdate(body, "r1", { last_name: "Lee" });
        expect(body.update).toEqual([
            { id: "r1", first_name: "Ava", last_name: "Lee" },
        ]);
    });

    it("delete cancels a pending update and dedupes", () => {
        const body = seeded();
        upsertUpdate(body, "r1", { first_name: "Ava" });
        addDelete(body, "r1");
        addDelete(body, "r1");
        expect(body.update).toHaveLength(0);
        expect(body.delete).toEqual(["r1"]);
    });

    it("removes an update without deleting", () => {
        const body = seeded();
        upsertUpdate(body, "r1", { first_name: "Ava" });
        removeUpdate(body, "r1");
        expect(body.update).toHaveLength(0);
        expect(body.delete).toHaveLength(0);
    });
});

describe("serializeRelationBody", () => {
    it("keys by child collection and strips the FK", () => {
        const body = seeded();
        addCreate(body, { first_name: "Ava", customer: "parent-1" });
        upsertUpdate(body, "r1", { first_name: "Leo", customer: "parent-1" });
        addDelete(body, "r2");
        expect(serializeRelationBody(body, "contacts", "customer")).toEqual({
            contacts: {
                create: [{ first_name: "Ava" }],
                update: [{ id: "r1", first_name: "Leo" }],
                delete: ["r2"],
            },
        });
    });

    it("preserves nested grandchild bodies in create values", () => {
        const body = seeded();
        addCreate(body, {
            first_name: "Ava",
            orders: { create: [{ total: 5 }] },
        });
        expect(serializeRelationBody(body, "contacts")?.contacts.create).toEqual(
            [{ first_name: "Ava", orders: { create: [{ total: 5 }] } }],
        );
    });

    it("returns null when empty or missing child collection", () => {
        expect(serializeRelationBody(seeded(), "contacts")).toBeNull();
        const body = seeded();
        addCreate(body, { a: 1 });
        expect(serializeRelationBody(body, "")).toBeNull();
    });
});

describe("mergeRelationBodies", () => {
    it("merges create/update/delete arrays per collection", () => {
        const merged = mergeRelationBodies(
            { contacts: { create: [{ a: 1 }], delete: ["x"] } },
            {
                contacts: { create: [{ b: 2 }], update: [{ id: "r1" }] },
                notes: { create: [{ c: 3 }] },
            },
        );
        expect(merged).toEqual({
            contacts: {
                create: [{ a: 1 }, { b: 2 }],
                update: [{ id: "r1" }],
                delete: ["x"],
            },
            notes: { create: [{ c: 3 }] },
        });
    });

    it("is a no-op for null source", () => {
        const target = { contacts: { create: [{ a: 1 }] } };
        expect(mergeRelationBodies(target, null)).toEqual(target);
    });
});
