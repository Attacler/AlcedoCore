/**
 * Shared section layout utilities extracted from RecordForm.vue and RecordDetail.vue.
 *
 * Both components had near-identical implementations of:
 *   - normalizeSection()  — hydrates raw section data with _columns / _field_columns layout info
 *   - getColumnFields()   — filters a field list by column index based on _field_columns config
 */

export function useSectionLayout() {
  /**
   * Normalize a raw section object by hydrating layout metadata
   * (_columns and _field_columns) from default_filter.
   */
  function normalizeSection(s: any): any {
    let _columns = 1
    let _field_columns: Record<string, number> = {}
    if (s.default_filter && typeof s.default_filter === 'object') {
      _columns = s.default_filter._columns || 1
      if (s.default_filter._field_columns) {
        _field_columns = s.default_filter._field_columns
      }
    }
    return {
      ...s,
      section_type: s.section_type || 'relational',
      display_fields: s.display_fields || [],
      _columns,
      _field_columns,
    }
  }

  return { normalizeSection }
}

/**
 * Parse a relational section's relation_field into its parts.
 *
 * Namespaced format: "<collection>.<field>" (e.g. "contacts.customer").
 * Legacy format: a bare field name whose related_collection lives on the
 * current (parent) collection.
 */
export function parseRelationField(
  relationField: string | null | undefined,
): { collection: string | null; field: string } | null {
  if (!relationField) return null
  const idx = relationField.indexOf('.')
  if (idx > 0 && idx < relationField.length - 1) {
    return {
      collection: relationField.slice(0, idx),
      field: relationField.slice(idx + 1),
    }
  }
  return { collection: null, field: relationField }
}

/**
 * Resolve the child collection name for a relational section.
 *
 * - Namespaced "collection.field" → the collection that owns the relation field.
 * - Legacy bare field name → the parent field's related_collection.
 */
export function getSectionChildCollectionName(
  relationField: string | null | undefined,
  parentFields: { name: string; related_collection?: string | null }[],
): string {
  const parsed = parseRelationField(relationField)
  if (!parsed) return ''
  if (parsed.collection) return parsed.collection
  const parentField = parentFields.find((f) => f.name === parsed.field)
  return parentField?.related_collection || ''
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
export function getColumnFields(section: any, colIdx: number, fieldList: any[]): any[] {
  const fieldColumns = section._field_columns || {}
  return fieldList.filter((f: any) => {
    const col = fieldColumns[f.name]
    return col === undefined ? colIdx === 0 : col === colIdx + 1
  })
}
