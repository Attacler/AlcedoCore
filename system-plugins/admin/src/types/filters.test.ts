import { describe, it, expect } from "vitest";
import {
    toShortForm,
    normalizeFilterCondition,
    createEmptyRule,
    hasNonEmptyCondition,
} from "./filters";
import type { FilterCondition, FilterRule } from "./filters";

describe("toShortForm", () => {
    it("nests a relation path", () => {
        const rule: FilterRule = {
            path: ["customer", "name"],
            operator: "eq",
            value: "x",
        };
        expect(toShortForm(rule)).toEqual({
            customer: { name: { _eq: "x" } },
        });
    });

    it("keeps a plain field flat", () => {
        const rule: FilterRule = {
            path: ["status"],
            operator: "eq",
            value: "open",
        };
        expect(toShortForm(rule)).toEqual({ status: { _eq: "open" } });
    });

    it("nests deep paths and maps operators", () => {
        const cond: FilterCondition = {
            operator: "and",
            conditions: [
                { path: ["tickets", "status"], operator: "not_null", value: null },
                { path: ["contacts", "is_primary"], operator: "eq", value: true },
            ],
        };
        expect(toShortForm(cond)).toEqual({
            _and: [
                { tickets: { status: { _nnull: null } } },
                { contacts: { is_primary: { _eq: true } } },
            ],
        });
    });
});

describe("normalizeFilterCondition", () => {
    it("converts a legacy dotted field to a path", () => {
        const legacy = {
            operator: "and",
            conditions: [
                { field: "tickets.status", operator: "eq", value: "open" },
            ],
        } as unknown as FilterCondition;
        expect(normalizeFilterCondition(legacy)).toEqual({
            operator: "and",
            conditions: [
                { path: ["tickets", "status"], operator: "eq", value: "open" },
            ],
        });
    });

    it("leaves path-based conditions unchanged", () => {
        const cond: FilterCondition = {
            path: ["a", "b"],
            operator: "eq",
            value: 1,
        };
        expect(normalizeFilterCondition(cond)).toEqual(cond);
    });

    it("returns null for empty input", () => {
        expect(normalizeFilterCondition(null)).toBeNull();
        expect(normalizeFilterCondition(undefined)).toBeNull();
    });
});

describe("hasNonEmptyCondition", () => {
    it("is false for an empty rule", () => {
        expect(hasNonEmptyCondition(createEmptyRule())).toBe(false);
    });

    it("is true once a path is set", () => {
        expect(
            hasNonEmptyCondition({ path: ["x"], operator: "eq", value: "" }),
        ).toBe(true);
    });
});
