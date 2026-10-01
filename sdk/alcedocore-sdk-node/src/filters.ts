/**
 * Client-side evaluation of AlcedoCore permission filters.
 *
 * A stored `field_validation` (create) or `filter` (read/delete) rule is a flat
 * array of conditions. This module answers "would this object pass?" without a
 * round trip, so UIs can prefill/validate before submitting.
 *
 * Semantics are kept deliberately close to the server (`corev2`
 * `services/permissions/create.rs`):
 *  - conditions within a rule are AND-ed;
 *  - a rule with no conditions is always satisfied (used as the `rules` OR);
 *  - a field that is absent from `item` is skipped, because the column falls
 *    back to its database default (v1 parity) — use an `{ operator: "null" }`
 *    condition to require an explicit null;
 *  - only scalar conditions are evaluated. A dot-notation path (a relation)
 *    cannot be checked against an in-memory object, so it is reported as
 *    `unknown` and skipped rather than silently passing.
 *
 * Literal values are compared as-is. `{user.*}` placeholders are resolved
 * server-side and never appear in a summary/filter fetched by a client; if one
 * is present it compares literally.
 */

/** One flat permission condition, in the shape the API stores. */
export interface FilterCondition {
    field: string;
    /** Operator long form (`eq`, `in`, `contains`, ...). Defaults to `eq`. */
    operator?: string;
    value?: unknown;
}

export type Filter = FilterCondition[];

/** How a single condition was decided. */
export type ConditionStatus = "match" | "mismatch" | "unknown";

/** The outcome of evaluating a filter against one item. */
export interface FilterResult {
    /** True when every evaluable condition matched. */
    matched: boolean;
    /** Per-condition detail, in the order the conditions were given. */
    results: ConditionStatus[];
    /** Conditions that could not be evaluated (dot-notation relations). */
    unknown: FilterCondition[];
}

/** A JSON scalar (anything that is not a nested object or array). */
type Scalar = string | number | boolean | null;

/**
 * Reads `field` from `item`, descending nested objects for dot-notation paths.
 * Returns `undefined` when any segment is missing.
 */
function resolvePath(item: unknown, field: string): unknown {
    let current = item;
    for (const segment of field.split(".")) {
        if (current === null || typeof current !== "object" || Array.isArray(current)) {
            return undefined;
        }
        current = (current as Record<string, unknown>)[segment];
    }
    return current;
}

/** True when `field` needs relation resolution that a client cannot perform. */
function isRelationPath(field: string): boolean {
    return field.includes(".");
}

/** Numeric-aware equality: `1` equals `1.0`, `"1"` does not equal `1`. */
function looseEquals(actual: unknown, expected: unknown): boolean {
    if (typeof actual === "number" && typeof expected === "number") {
        return actual === expected;
    }
    return actual === expected;
}

/** String form used by `contains`/`starts_with`/`ends_with`. */
function text(value: unknown): string {
    return typeof value === "string" ? value : String(value);
}

/** Numeric comparison; falls back to lexicographic for non-numbers. */
function compare(actual: unknown, expected: unknown): number | undefined {
    if (typeof actual === "number" && typeof expected === "number") {
        return actual === expected ? 0 : actual < expected ? -1 : 1;
    }
    if (actual == null || expected == null) {
        return undefined;
    }
    const left = text(actual);
    const right = text(expected);
    return left === right ? 0 : left < right ? -1 : 1;
}

/** Evaluates one non-relation condition against a present value. */
function matchesSingle(operator: string, expected: unknown, actual: unknown): boolean {
    // `in`/`nin` short-circuit before the field is resolved, matching the server
    // (an empty list rejects for `in` and accepts for `nin`, present or not).
    if (operator === "in" && Array.isArray(expected) && expected.length === 0) {
        return false;
    }
    if (operator === "nin" && Array.isArray(expected) && expected.length === 0) {
        return true;
    }
    switch (operator) {
        case "eq":
            return looseEquals(actual, expected);
        case "neq":
        case "not_eq":
            return !looseEquals(actual, expected);
        case "gt":
            return (compare(actual, expected) ?? 0) > 0;
        case "gte":
            return (compare(actual, expected) ?? -1) >= 0;
        case "lt":
            return (compare(actual, expected) ?? 0) < 0;
        case "lte":
            return (compare(actual, expected) ?? 1) <= 0;
        case "in":
            return Array.isArray(expected) && expected.some((value) => looseEquals(actual, value));
        case "nin":
        case "not_in":
            return !Array.isArray(expected) || !expected.some((value) => looseEquals(actual, value));
        case "null":
        case "is_null":
            return actual === null;
        case "nnull":
        case "not_null":
            return actual !== null;
        case "contains":
            return text(actual).includes(text(expected));
        case "ncontains":
        case "not_contains":
            return !text(actual).includes(text(expected));
        case "icontains":
            return text(actual).toLowerCase().includes(text(expected).toLowerCase());
        case "nicontains":
        case "not_icontains":
            return !text(actual).toLowerCase().includes(text(expected).toLowerCase());
        case "starts_with":
            return text(actual).startsWith(text(expected));
        case "istarts_with":
            return text(actual).toLowerCase().startsWith(text(expected).toLowerCase());
        case "nstarts_with":
        case "not_starts_with":
            return !text(actual).startsWith(text(expected));
        case "nistarts_with":
        case "not_istarts_with":
            return !text(actual).toLowerCase().startsWith(text(expected).toLowerCase());
        case "ends_with":
            return text(actual).endsWith(text(expected));
        case "iends_with":
            return text(actual).toLowerCase().endsWith(text(expected).toLowerCase());
        case "nends_with":
        case "not_ends_with":
            return !text(actual).endsWith(text(expected));
        case "niends_with":
        case "not_iends_with":
            return !text(actual).toLowerCase().endsWith(text(expected).toLowerCase());
        case "between":
        case "nbetween":
        case "not_between": {
            const pair = Array.isArray(expected) ? expected : [];
            const within =
                pair.length >= 2 &&
                (compare(actual, pair[0]) ?? -1) >= 0 &&
                (compare(actual, pair[1]) ?? 1) <= 0;
            return operator === "between" ? within : !within;
        }
        default:
            return false;
    }
}

/** The operators this util understands (long form + the aliases the API keeps). */
export const KNOWN_OPERATORS: ReadonlySet<string> = new Set([
    "eq",
    "neq",
    "not_eq",
    "gt",
    "gte",
    "lt",
    "lte",
    "in",
    "nin",
    "not_in",
    "null",
    "is_null",
    "nnull",
    "not_null",
    "contains",
    "ncontains",
    "not_contains",
    "icontains",
    "nicontains",
    "not_icontains",
    "starts_with",
    "istarts_with",
    "nstarts_with",
    "not_starts_with",
    "nistarts_with",
    "not_istarts_with",
    "ends_with",
    "iends_with",
    "nends_with",
    "not_ends_with",
    "niends_with",
    "not_iends_with",
    "between",
    "nbetween",
    "not_between",
]);

/**
 * Evaluates one condition against `item`, reporting whether it could be decided.
 */
export function evaluateCondition(condition: FilterCondition, item: unknown): ConditionStatus {
    const operator = condition.operator ?? "eq";
    if (isRelationPath(condition.field) || !KNOWN_OPERATORS.has(operator)) {
        return "unknown";
    }
    const expected = condition.value;
    const actual = resolvePath(item, condition.field);
    // `in`/`nin` are decided before the field is resolved, mirroring the server:
    // an empty list rejects for `in` and accepts for `nin`, present or not.
    if (operator === "in" && Array.isArray(expected) && expected.length === 0) {
        return "mismatch";
    }
    if (operator === "nin" && Array.isArray(expected) && expected.length === 0) {
        return "match";
    }
    // Absent field: the column falls back to its database default, so the
    // condition is not enforced client-side.
    if (actual === undefined) {
        return "unknown";
    }
    return matchesSingle(operator, expected, actual) ? "match" : "mismatch";
}

/**
 * Evaluates a whole filter (AND-ed conditions) against `item`.
 *
 * ```ts
 * evaluateFilter(item, [{ field: "status", operator: "eq", value: "open" }]).matched;
 * ```
 */
export function evaluateFilter(filter: Filter | null | undefined, item: unknown): FilterResult {
    const conditions = Array.isArray(filter) ? filter : [];
    const results: ConditionStatus[] = [];
    const unknown: FilterCondition[] = [];

    for (const condition of conditions) {
        const status = evaluateCondition(condition, item);
        results.push(status);
        if (status === "unknown") {
            unknown.push(condition);
        }
    }

    return {
        matched: results.every((status) => status !== "mismatch"),
        results,
        unknown,
    };
}

/** Evaluates a single filter and returns just the boolean verdict. */
export function matchesFilter(filter: Filter | null | undefined, item: unknown): boolean {
    return evaluateFilter(filter, item).matched;
}

/**
 * Evaluates several rules (the OR-ed combination the API returns for a policy).
 * An empty rule list means "no evaluable rules" — the caller decides the default.
 */
export function matchesAnyFilter(filters: Array<Filter | null | undefined>, item: unknown): boolean {
    return filters.some((filter) => matchesFilter(filter, item));
}

/**
 * Builds an item from the fields a set of `eq` conditions pins to a single
 * value (e.g. a create form's `field_validation`). Used to prefill a form.
 * Non-`eq` conditions, relation paths and repeated/conflicting fields are
 * skipped.
 */
export function proposedValues(filter: Filter | null | undefined): Record<string, Scalar> {
    const values: Record<string, Scalar> = {};
    for (const condition of filter ?? []) {
        const operator = condition.operator ?? "eq";
        if (operator !== "eq" || isRelationPath(condition.field)) {
            continue;
        }
        const value = condition.value;
        if (value === undefined || (typeof value === "object" && value !== null)) {
            continue;
        }
        const existing = values[condition.field];
        if (existing === undefined) {
            values[condition.field] = value as Scalar;
        }
    }
    return values;
}
