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
