import type { FieldDefinition } from "@/stores/collections";
import type { FilterCondition } from "@/types/filters";
import { isFilterGroup, isFilterRule } from "@/types/filters";

export interface ClientDataSourceParams {
    page: number;
    perPage: number;
    sortField?: string;
    sortOrder?: "asc" | "desc";
    filterCondition?: FilterCondition | null;
}

export interface CollectionDataSource {
    fields: FieldDefinition[];
    fetch(
        params: ClientDataSourceParams,
    ): Promise<{ items: any[]; total: number }>;
}
// TODO: This should be done by the API not clientsided
/** Lazy-loaded, client-side data source backed by a store's already-loaded rows. */
export function createClientDataSource(opts: {
    load: () => Promise<void>;
    getRows: () => any[];
    fields: FieldDefinition[];
}): CollectionDataSource {
    let loaded = false;

    return {
        fields: opts.fields,
        async fetch({ page, perPage, sortField, sortOrder, filterCondition }) {
            if (!loaded) {
                await opts.load();
                loaded = true;
            }
            let rows = opts.getRows().slice();
            if (filterCondition) {
                rows = rows.filter((item) =>
                    matchesFilter(filterCondition, item),
                );
            }
            if (sortField) {
                rows.sort((a, b) => {
                    const av = a[sortField];
                    const bv = b[sortField];
                    const cmp =
                        av === bv
                            ? 0
                            : av == null
                              ? 1
                              : bv == null
                                ? -1
                                : String(av) < String(bv)
                                  ? -1
                                  : 1;
                    return sortOrder === "desc" ? -cmp : cmp;
                });
            }
            const total = rows.length;
            const start = (page - 1) * perPage;
            return { items: rows.slice(start, start + perPage), total };
        },
    };
}

/** Evaluate a structured FilterCondition against a single row. */
export function matchesFilter(cond: FilterCondition, item: any): boolean {
    if (isFilterGroup(cond)) {
        const results = cond.conditions.map((c) => matchesFilter(c, item));
        return cond.operator === "and"
            ? results.every(Boolean)
            : results.some(Boolean);
    }
    if (!isFilterRule(cond)) return true;
    const val = item[cond.field];
    const expected = cond.value;
    switch (cond.operator) {
        case "eq":
            return String(val ?? "") === String(expected ?? "");
        case "neq":
            return String(val ?? "") !== String(expected ?? "");
        case "gt":
            return Number(val) > Number(expected);
        case "gte":
            return Number(val) >= Number(expected);
        case "lt":
            return Number(val) < Number(expected);
        case "lte":
            return Number(val) <= Number(expected);
        case "contains":
            return String(val ?? "")
                .toLowerCase()
                .includes(String(expected ?? "").toLowerCase());
        case "starts_with":
            return String(val ?? "")
                .toLowerCase()
                .startsWith(String(expected ?? "").toLowerCase());
        case "ends_with":
            return String(val ?? "")
                .toLowerCase()
                .endsWith(String(expected ?? "").toLowerCase());
        case "in":
            return (
                Array.isArray(expected) &&
                expected.map(String).includes(String(val ?? ""))
            );
        case "not_in":
            return (
                Array.isArray(expected) &&
                !expected.map(String).includes(String(val ?? ""))
            );
        case "null":
            return val === null || val === undefined || val === "";
        case "not_null":
            return val !== null && val !== undefined && val !== "";
        default:
            return true;
    }
}
