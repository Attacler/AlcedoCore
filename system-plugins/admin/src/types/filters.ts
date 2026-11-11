/**
 * Filter types for the FilterBuilder component.
 *
 * These types match the FilterCondition JSON format expected by
 * the backend query_builder (Phase 35) — used by POST /api/collections/:name/items/query.
 */

export type FilterOperator =
    | "eq"
    | "neq"
    | "gt"
    | "gte"
    | "lt"
    | "lte"
    | "contains"
    | "starts_with"
    | "ends_with"
    | "in"
    | "not_in"
    | "null"
    | "not_null"
    | "count_eq"
    | "count_neq"
    | "count_gt"
    | "count_gte"
    | "count_lt"
    | "count_lte"
    | "sum_eq"
    | "sum_neq"
    | "sum_gt"
    | "sum_gte"
    | "sum_lt"
    | "sum_lte"
    | "sum_distinct_eq"
    | "sum_distinct_neq"
    | "sum_distinct_gt"
    | "sum_distinct_gte"
    | "sum_distinct_lt"
    | "sum_distinct_lte"
    | "avg_eq"
    | "avg_neq"
    | "avg_gt"
    | "avg_gte"
    | "avg_lt"
    | "avg_lte"
    | "avg_distinct_eq"
    | "avg_distinct_neq"
    | "avg_distinct_gt"
    | "avg_distinct_gte"
    | "avg_distinct_lt"
    | "avg_distinct_lte"
    | "min_eq"
    | "min_neq"
    | "min_gt"
    | "min_gte"
    | "min_lt"
    | "min_lte"
    | "max_eq"
    | "max_neq"
    | "max_gt"
    | "max_gte"
    | "max_lt"
    | "max_lte";

export type GroupOperator = "and" | "or";

export interface FilterRule {
    path: string[];
    operator: FilterOperator;
    value: unknown;
}

/** A recursive filter group with AND/OR logic */
export interface FilterGroup {
    operator: GroupOperator;
    conditions: FilterCondition[];
}

/** A filter condition is either a leaf rule or a nested group */
export type FilterCondition = FilterRule | FilterGroup;

/** Top-level query payload sent to the backend */
export interface FilterPayload {
    filter: FilterCondition;
}

/** Operator metadata: which field types support it, and value requirement */
export interface OperatorMeta {
    operator: FilterOperator;
    label: string;
    requiresValue: boolean;
}

/** Operators available per field type */
export const OPERATORS_BY_TYPE: Record<string, OperatorMeta[]> = {
    string: [
        { operator: "eq", label: "Equals", requiresValue: true },
        { operator: "neq", label: "Not equals", requiresValue: true },
        { operator: "contains", label: "Contains", requiresValue: true },
        { operator: "starts_with", label: "Starts with", requiresValue: true },
        { operator: "ends_with", label: "Ends with", requiresValue: true },
        { operator: "null", label: "Is null", requiresValue: false },
        { operator: "not_null", label: "Is not null", requiresValue: false },
    ],
    text: [
        { operator: "eq", label: "Equals", requiresValue: true },
        { operator: "neq", label: "Not equals", requiresValue: true },
        { operator: "contains", label: "Contains", requiresValue: true },
        { operator: "starts_with", label: "Starts with", requiresValue: true },
        { operator: "ends_with", label: "Ends with", requiresValue: true },
        { operator: "null", label: "Is null", requiresValue: false },
        { operator: "not_null", label: "Is not null", requiresValue: false },
    ],
    int: [
        { operator: "eq", label: "Equals", requiresValue: true },
        { operator: "neq", label: "Not equals", requiresValue: true },
        { operator: "gt", label: "Greater than", requiresValue: true },
        { operator: "gte", label: "Greater or equal", requiresValue: true },
        { operator: "lt", label: "Less than", requiresValue: true },
        { operator: "lte", label: "Less or equal", requiresValue: true },
        { operator: "in", label: "In list", requiresValue: true },
        { operator: "not_in", label: "Not in list", requiresValue: true },
        { operator: "null", label: "Is null", requiresValue: false },
        { operator: "not_null", label: "Is not null", requiresValue: false },
    ],
    float: [
        { operator: "eq", label: "Equals", requiresValue: true },
        { operator: "neq", label: "Not equals", requiresValue: true },
        { operator: "gt", label: "Greater than", requiresValue: true },
        { operator: "gte", label: "Greater or equal", requiresValue: true },
        { operator: "lt", label: "Less than", requiresValue: true },
        { operator: "lte", label: "Less or equal", requiresValue: true },
        { operator: "in", label: "In list", requiresValue: true },
        { operator: "not_in", label: "Not in list", requiresValue: true },
        { operator: "null", label: "Is null", requiresValue: false },
        { operator: "not_null", label: "Is not null", requiresValue: false },
    ],
    datetime: [
        { operator: "eq", label: "Equals", requiresValue: true },
        { operator: "neq", label: "Not equals", requiresValue: true },
        { operator: "gt", label: "After", requiresValue: true },
        { operator: "gte", label: "After or equal", requiresValue: true },
        { operator: "lt", label: "Before", requiresValue: true },
        { operator: "lte", label: "Before or equal", requiresValue: true },
        { operator: "null", label: "Is null", requiresValue: false },
        { operator: "not_null", label: "Is not null", requiresValue: false },
    ],
    uuid: [
        { operator: "eq", label: "Equals", requiresValue: true },
        { operator: "neq", label: "Not equals", requiresValue: true },
        { operator: "in", label: "In list", requiresValue: true },
        { operator: "not_in", label: "Not in list", requiresValue: true },
        { operator: "null", label: "Is null", requiresValue: false },
        { operator: "not_null", label: "Is not null", requiresValue: false },
    ],
    relationship: [
        { operator: "eq", label: "Equals", requiresValue: true },
        { operator: "neq", label: "Not equals", requiresValue: true },
        { operator: "in", label: "In list", requiresValue: true },
        { operator: "not_in", label: "Not in list", requiresValue: true },
        { operator: "null", label: "Is null", requiresValue: false },
        { operator: "not_null", label: "Is not null", requiresValue: false },
    ],
};

/**
 * Operators for virtual one-to-many relation fields (e.g. `contacts` on
 * `customers`): "parents with N related rows". Sent as `_count_*` and
 * resolved by the backend as a COUNT(*) subquery.
 */
export const COUNT_OPERATORS: OperatorMeta[] = [
    { operator: "count_eq", label: "Count =", requiresValue: true },
    { operator: "count_neq", label: "Count ≠", requiresValue: true },
    { operator: "count_gt", label: "Count >", requiresValue: true },
    { operator: "count_gte", label: "Count ≥", requiresValue: true },
    { operator: "count_lt", label: "Count <", requiresValue: true },
    { operator: "count_lte", label: "Count ≤", requiresValue: true },
];

/**
 * Aggregate operators for child fields of a virtual one-to-many relation
 * (e.g. `lines.total` on `invoices`): "parents whose related rows sum to
 * more than N". Sent as `_sum_*` / `_avg_*` / `_min_*` / `_max_*` and
 * resolved by the backend as an aggregate subquery.
 */
export const AGGREGATE_OPERATORS: OperatorMeta[] = [
    { operator: "sum_eq", label: "Sum =", requiresValue: true },
    { operator: "sum_neq", label: "Sum ≠", requiresValue: true },
    { operator: "sum_gt", label: "Sum >", requiresValue: true },
    { operator: "sum_gte", label: "Sum ≥", requiresValue: true },
    { operator: "sum_lt", label: "Sum <", requiresValue: true },
    { operator: "sum_lte", label: "Sum ≤", requiresValue: true },
    { operator: "sum_distinct_eq", label: "Sum distinct =", requiresValue: true },
    { operator: "sum_distinct_neq", label: "Sum distinct ≠", requiresValue: true },
    { operator: "sum_distinct_gt", label: "Sum distinct >", requiresValue: true },
    { operator: "sum_distinct_gte", label: "Sum distinct ≥", requiresValue: true },
    { operator: "sum_distinct_lt", label: "Sum distinct <", requiresValue: true },
    { operator: "sum_distinct_lte", label: "Sum distinct ≤", requiresValue: true },
    { operator: "avg_eq", label: "Avg =", requiresValue: true },
    { operator: "avg_neq", label: "Avg ≠", requiresValue: true },
    { operator: "avg_gt", label: "Avg >", requiresValue: true },
    { operator: "avg_gte", label: "Avg ≥", requiresValue: true },
    { operator: "avg_lt", label: "Avg <", requiresValue: true },
    { operator: "avg_lte", label: "Avg ≤", requiresValue: true },
    { operator: "avg_distinct_eq", label: "Avg distinct =", requiresValue: true },
    { operator: "avg_distinct_neq", label: "Avg distinct ≠", requiresValue: true },
    { operator: "avg_distinct_gt", label: "Avg distinct >", requiresValue: true },
    { operator: "avg_distinct_gte", label: "Avg distinct ≥", requiresValue: true },
    { operator: "avg_distinct_lt", label: "Avg distinct <", requiresValue: true },
    { operator: "avg_distinct_lte", label: "Avg distinct ≤", requiresValue: true },
    { operator: "min_eq", label: "Min =", requiresValue: true },
    { operator: "min_neq", label: "Min ≠", requiresValue: true },
    { operator: "min_gt", label: "Min >", requiresValue: true },
    { operator: "min_gte", label: "Min ≥", requiresValue: true },
    { operator: "min_lt", label: "Min <", requiresValue: true },
    { operator: "min_lte", label: "Min ≤", requiresValue: true },
    { operator: "max_eq", label: "Max =", requiresValue: true },
    { operator: "max_neq", label: "Max ≠", requiresValue: true },
    { operator: "max_gt", label: "Max >", requiresValue: true },
    { operator: "max_gte", label: "Max ≥", requiresValue: true },
    { operator: "max_lt", label: "Max <", requiresValue: true },
    { operator: "max_lte", label: "Max ≤", requiresValue: true },
];

/** Check if a condition is a group (has `conditions` property) */
export function isFilterGroup(
    condition: FilterCondition,
): condition is FilterGroup {
    return "conditions" in condition;
}

export function isFilterRule(
    condition: FilterCondition,
): condition is FilterRule {
    return "path" in condition;
}

/** Recursively check if a filter condition has any meaningful rules */
export function hasNonEmptyCondition(
    cond: FilterCondition | null | undefined,
): boolean {
    if (!cond) return false;
    if (isFilterRule(cond)) return !!cond.path && cond.path.length > 0;
    if (cond.conditions.length > 0) {
        return cond.conditions.some((c) => hasNonEmptyCondition(c));
    }
    return false;
}

/** Create an empty rule with sensible defaults */
export function createEmptyRule(): FilterRule {
    return { path: [], operator: "eq", value: "" };
}

/** Create an empty group with AND operator */
export function createEmptyGroup(): FilterGroup {
    return { operator: "and", conditions: [] };
}

// ---------------------------------------------------------------------------
// Short-form JSON conversion (matching the Filter API spec)
// ---------------------------------------------------------------------------

/** Map internal operator names to short-form (with _ prefix) */
const OPERATOR_TO_SHORT: Record<string, string> = {
    eq: "_eq",
    neq: "_neq",
    gt: "_gt",
    gte: "_gte",
    lt: "_lt",
    lte: "_lte",
    contains: "_contains",
    starts_with: "_starts_with",
    ends_with: "_ends_with",
    in: "_in",
    not_in: "_nin",
    null: "_null",
    not_null: "_nnull",
    count_eq: "_count_eq",
    count_neq: "_count_neq",
    count_gt: "_count_gt",
    count_gte: "_count_gte",
    count_lt: "_count_lt",
    count_lte: "_count_lte",
    sum_eq: "_sum_eq",
    sum_neq: "_sum_neq",
    sum_gt: "_sum_gt",
    sum_gte: "_sum_gte",
    sum_lt: "_sum_lt",
    sum_lte: "_sum_lte",
    sum_distinct_eq: "_sum_distinct_eq",
    sum_distinct_neq: "_sum_distinct_neq",
    sum_distinct_gt: "_sum_distinct_gt",
    sum_distinct_gte: "_sum_distinct_gte",
    sum_distinct_lt: "_sum_distinct_lt",
    sum_distinct_lte: "_sum_distinct_lte",
    avg_eq: "_avg_eq",
    avg_neq: "_avg_neq",
    avg_gt: "_avg_gt",
    avg_gte: "_avg_gte",
    avg_lt: "_avg_lt",
    avg_lte: "_avg_lte",
    avg_distinct_eq: "_avg_distinct_eq",
    avg_distinct_neq: "_avg_distinct_neq",
    avg_distinct_gt: "_avg_distinct_gt",
    avg_distinct_gte: "_avg_distinct_gte",
    avg_distinct_lt: "_avg_distinct_lt",
    avg_distinct_lte: "_avg_distinct_lte",
    min_eq: "_min_eq",
    min_neq: "_min_neq",
    min_gt: "_min_gt",
    min_gte: "_min_gte",
    min_lt: "_min_lt",
    min_lte: "_min_lte",
    max_eq: "_max_eq",
    max_neq: "_max_neq",
    max_gt: "_max_gt",
    max_gte: "_max_gte",
    max_lt: "_max_lt",
    max_lte: "_max_lte",
};

/** Map short-form operator names back to internal names */
const SHORT_TO_OPERATOR: Record<string, string> = {
    _eq: "eq",
    _neq: "neq",
    _gt: "gt",
    _gte: "gte",
    _lt: "lt",
    _lte: "lte",
    _contains: "contains",
    _starts_with: "starts_with",
    _ends_with: "ends_with",
    _in: "in",
    _nin: "not_in",
    _null: "null",
    _nnull: "not_null",
    _count_eq: "count_eq",
    _count_neq: "count_neq",
    _count_gt: "count_gt",
    _count_gte: "count_gte",
    _count_lt: "count_lt",
    _count_lte: "count_lte",
    _sum_eq: "sum_eq",
    _sum_neq: "sum_neq",
    _sum_gt: "sum_gt",
    _sum_gte: "sum_gte",
    _sum_lt: "sum_lt",
    _sum_lte: "sum_lte",
    _sum_distinct_eq: "sum_distinct_eq",
    _sum_distinct_neq: "sum_distinct_neq",
    _sum_distinct_gt: "sum_distinct_gt",
    _sum_distinct_gte: "sum_distinct_gte",
    _sum_distinct_lt: "sum_distinct_lt",
    _sum_distinct_lte: "sum_distinct_lte",
    _avg_eq: "avg_eq",
    _avg_neq: "avg_neq",
    _avg_gt: "avg_gt",
    _avg_gte: "avg_gte",
    _avg_lt: "avg_lt",
    _avg_lte: "avg_lte",
    _avg_distinct_eq: "avg_distinct_eq",
    _avg_distinct_neq: "avg_distinct_neq",
    _avg_distinct_gt: "avg_distinct_gt",
    _avg_distinct_gte: "avg_distinct_gte",
    _avg_distinct_lt: "avg_distinct_lt",
    _avg_distinct_lte: "avg_distinct_lte",
    _min_eq: "min_eq",
    _min_neq: "min_neq",
    _min_gt: "min_gt",
    _min_gte: "min_gte",
    _min_lt: "min_lt",
    _min_lte: "min_lte",
    _max_eq: "max_eq",
    _max_neq: "max_neq",
    _max_gt: "max_gt",
    _max_gte: "max_gte",
    _max_lt: "max_lt",
    _max_lte: "max_lte",
};

function nestPath(path: string[], leaf: unknown): Record<string, unknown> {
    let node: unknown = leaf;
    for (let i = path.length - 1; i >= 0; i--) {
        node = { [path[i]]: node };
    }
    return node as Record<string, unknown>;
}

export function toShortForm(cond: FilterCondition): Record<string, unknown> {
    if (isFilterGroup(cond)) {
        const key = cond.operator === "and" ? "_and" : "_or";
        return { [key]: cond.conditions.map(toShortForm) };
    }
    const rule = cond as FilterRule;
    const opKey = OPERATOR_TO_SHORT[rule.operator] || rule.operator;
    const val =
        rule.value !== undefined && rule.value !== null && rule.value !== ""
            ? rule.value
            : null;
    return nestPath(rule.path || [], { [opKey]: val });
}

export function normalizeFilterCondition(
    cond: FilterCondition | null | undefined,
): FilterCondition | null {
    if (!cond) return null;
    if (isFilterGroup(cond)) {
        return {
            operator: cond.operator,
            conditions: cond.conditions
                .map((c) => normalizeFilterCondition(c))
                .filter((c): c is FilterCondition => !!c),
        };
    }
    const legacy = cond as unknown as {
        path?: unknown;
        field?: unknown;
        operator: FilterOperator;
        value: unknown;
    };
    const path: string[] = Array.isArray(legacy.path)
        ? (legacy.path as string[])
        : typeof legacy.field === "string" && legacy.field.length > 0
          ? legacy.field.split(".")
          : [];
    return { path, operator: legacy.operator, value: legacy.value };
}
