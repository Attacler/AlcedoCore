import { describe, it, expect } from "vitest";
import {
    evaluateCondition,
    evaluateFilter,
    matchesAnyFilter,
    matchesFilter,
    proposedValues,
} from "./filters.js";
import type { Filter } from "./filters.js";

const item = {
    status: "open",
    price: 10,
    type: "approved",
    customer: { name: "Acme Corp" },
};

describe("evaluateFilter", () => {
    it("matches on eq and mismatches on a different value", () => {
        expect(matchesFilter([{ field: "status", operator: "eq", value: "open" }], item)).toBe(true);
        expect(matchesFilter([{ field: "status", operator: "eq", value: "closed" }], item)).toBe(
            false,
        );
    });

    it("defaults the operator to eq", () => {
        expect(matchesFilter([{ field: "type", value: "approved" }], item)).toBe(true);
    });

    it("ANDs conditions together", () => {
        expect(
            matchesFilter(
                [
                    { field: "status", operator: "eq", value: "open" },
                    { field: "price", operator: "gt", value: 5 },
                ],
                item,
            ),
        ).toBe(true);
        expect(
            matchesFilter(
                [
                    { field: "status", operator: "eq", value: "open" },
                    { field: "price", operator: "gt", value: 50 },
                ],
                item,
            ),
        ).toBe(false);
    });

    it("reports a mismatched condition while still listing matches", () => {
        const result = evaluateFilter(
            [
                { field: "status", operator: "eq", value: "open" },
                { field: "price", operator: "lt", value: 5 },
            ],
            item,
        );
        expect(result.matched).toBe(false);
        expect(result.results).toEqual(["match", "mismatch"]);
    });

    it("treats an absent field as unknown and does not fail on it", () => {
        const result = evaluateFilter([{ field: "missing", operator: "eq", value: 1 }], item);
        expect(result.matched).toBe(true);
        expect(result.results).toEqual(["unknown"]);
        expect(result.unknown).toHaveLength(1);
    });

    it("enforces an empty in list even when the field is absent", () => {
        // Widening operators can fail even without a value; the short-circuits
        // mirror the server, which also rejects `in []` before resolving.
        expect(matchesFilter([{ field: "missing", operator: "in", value: [] }], item)).toBe(false);
    });

    it("skips relation paths as unknown", () => {
        const result = evaluateFilter(
            [{ field: "customer.name", operator: "eq", value: "Acme Corp" }],
            item,
        );
        expect(result.results).toEqual(["unknown"]);
        expect(result.matched).toBe(true);
    });

    it("skips unknown operators", () => {
        const result = evaluateFilter([{ field: "status", operator: "wat", value: 1 }], item);
        expect(result.results).toEqual(["unknown"]);
    });
});

describe("scalar operators", () => {
    const cases: Array<[string, unknown, unknown, boolean]> = [
        ["neq", 10, 10, false],
        ["not_eq", 10, 10, false],
        ["gte", 10, 10, true],
        ["lte", 10, 10, true],
        ["lt", 10, 11, true],
        ["in", 10, [1, 10], true],
        ["in", 10, [1, 2], false],
        ["nin", 10, [1, 2], true],
        ["not_in", 10, [1, 10], false],
        ["contains", "open", "pen", true],
        ["ncontains", "open", "pen", false],
        ["icontains", "open", "PEN", true],
        ["starts_with", "open", "op", true],
        ["ends_with", "open", "en", true],
        ["not_null", "open", undefined, true],
        ["null", "open", undefined, false],
    ];

    it.each(cases)("%s(%o, %o) === %s", (operator, actual, expected, expectedResult) => {
        const result = evaluateCondition({ field: "value", operator, value: expected }, { value: actual });
        expect(result).toBe(expectedResult ? "match" : "mismatch");
    });

    it("coerces numeric equality across int/float", () => {
        expect(matchesFilter([{ field: "price", operator: "eq", value: 10.0 }], item)).toBe(true);
    });

    it("handles between inclusively", () => {
        expect(matchesFilter([{ field: "price", operator: "between", value: [10, 20] }], item)).toBe(
            true,
        );
        expect(matchesFilter([{ field: "price", operator: "between", value: [11, 20] }], item)).toBe(
            false,
        );
    });
});

describe("matchesAnyFilter", () => {
    it("ORs the rules", () => {
        const rules: Filter[] = [
            [{ field: "type", operator: "eq", value: "rejected" }],
            [{ field: "status", operator: "eq", value: "open" }],
        ];
        expect(matchesAnyFilter(rules, item)).toBe(true);
    });

    it("is false when no rule matches", () => {
        expect(matchesAnyFilter([[{ field: "status", operator: "eq", value: "closed" }]], item)).toBe(
            false,
        );
    });

    it("accepts an empty rule list", () => {
        expect(matchesAnyFilter([], item)).toBe(false);
    });
});

describe("proposedValues", () => {
    it("collects the eq values", () => {
        expect(
            proposedValues([
                { field: "type", operator: "eq", value: "approved" },
                { field: "priority", operator: "eq", value: 2 },
            ]),
        ).toEqual({ type: "approved", priority: 2 });
    });

    it("skips non-eq, relation and object values", () => {
        expect(
            proposedValues([
                { field: "status", operator: "in", value: ["a", "b"] },
                { field: "customer.name", operator: "eq", value: "Acme" },
                { field: "meta", operator: "eq", value: { a: 1 } },
                { field: "keep", operator: "eq", value: 1 },
            ]),
        ).toEqual({ keep: 1 });
    });

    it("keeps the first value for a repeated field", () => {
        expect(
            proposedValues([
                { field: "type", operator: "eq", value: "approved" },
                { field: "type", operator: "eq", value: "rejected" },
            ]),
        ).toEqual({ type: "approved" });
    });

    it("returns an empty object for a missing filter", () => {
        expect(proposedValues(null)).toEqual({});
        expect(proposedValues(undefined)).toEqual({});
    });
});
