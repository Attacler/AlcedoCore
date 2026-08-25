/**
 * Normalize a raw section object by hydrating layout metadata
 * (_columns and _field_columns) from default_filter.
 */
export function normalizeSection(s: any): any {
    let _columns = 1;
    let _field_columns: Record<string, number> = {};
    if (s.default_filter && typeof s.default_filter === "object") {
        _columns = s.default_filter._columns || 1;
        if (s.default_filter._field_columns) {
            _field_columns = s.default_filter._field_columns;
        }
    }
    return {
        ...s,
        section_type: s.section_type || "relational",
        display_fields: s.display_fields || [],
        _columns,
        _field_columns,
    };
}

/**
 * Resolve the child collection name for a relational section.
 *
 * The relation_field is always namespaced "collection.field" — the child
 * collection is the part before the first ".".
 */
export function getSectionChildCollectionName(
    relationField: string | null | undefined,
): string {
    if (!relationField) return "";
    const idx = relationField.indexOf(".");
    if (idx > 0 && idx < relationField.length - 1) {
        return relationField.slice(0, idx);
    }
    return "";
}

/**
 * Filter a field list by column index.
 *
 * Uses section._field_columns to determine which column each field belongs to.
 * Fields without a column mapping default to column 0.
 *
 * @param section   - The section with _field_columns metadata
 * @param colIdx    - 0-based column index to filter for
 * @param fieldList - The pre-computed list of fields for this section
 */
export function getColumnFields(
    section: any,
    colIdx: number,
    fieldList: any[],
): any[] {
    // TODO: Clean up the sort/display and _field_columns part
    const fieldColumns = section._field_columns || {};
    return fieldList
        .filter((f: any) => {
            const col = fieldColumns[f.name];
            return col === undefined ? colIdx === 0 : col === colIdx + 1;
        })
        .sort((a, b) => {
            const aI = section.display_fields.findIndex((f) => f == a.name);
            const bI = section.display_fields.findIndex((f) => f == b.name);

            if (aI > bI) {
                return 1;
            }
            return -1;
        });
}
