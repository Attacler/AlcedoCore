/**
 * Filter types for the FilterBuilder component.
 *
 * These types match the FilterCondition JSON format expected by
 * the backend query_builder (Phase 35) — used by POST /api/collections/:name/items/query.
 */

export type FilterOperator =
  | 'eq'
  | 'neq'
  | 'gt'
  | 'gte'
  | 'lt'
  | 'lte'
  | 'contains'
  | 'starts_with'
  | 'ends_with'
  | 'in'
  | 'not_in'
  | 'null'
  | 'not_null'

export type GroupOperator = 'and' | 'or'

/** A single leaf filter rule: field + operator + value */
export interface FilterRule {
  field: string
  operator: FilterOperator
  value: unknown
}

/** A recursive filter group with AND/OR logic */
export interface FilterGroup {
  operator: GroupOperator
  conditions: FilterCondition[]
}

/** A filter condition is either a leaf rule or a nested group */
export type FilterCondition = FilterRule | FilterGroup

/** Top-level query payload sent to the backend */
export interface FilterPayload {
  filter: FilterCondition
}

/** Operator metadata: which field types support it, and value requirement */
export interface OperatorMeta {
  operator: FilterOperator
  label: string
  requiresValue: boolean
}

/** Operators available per field type */
export const OPERATORS_BY_TYPE: Record<string, OperatorMeta[]> = {
  string: [
    { operator: 'eq', label: 'Equals', requiresValue: true },
    { operator: 'neq', label: 'Not equals', requiresValue: true },
    { operator: 'contains', label: 'Contains', requiresValue: true },
    { operator: 'starts_with', label: 'Starts with', requiresValue: true },
    { operator: 'ends_with', label: 'Ends with', requiresValue: true },
    { operator: 'null', label: 'Is null', requiresValue: false },
    { operator: 'not_null', label: 'Is not null', requiresValue: false },
  ],
  text: [
    { operator: 'eq', label: 'Equals', requiresValue: true },
    { operator: 'neq', label: 'Not equals', requiresValue: true },
    { operator: 'contains', label: 'Contains', requiresValue: true },
    { operator: 'starts_with', label: 'Starts with', requiresValue: true },
    { operator: 'ends_with', label: 'Ends with', requiresValue: true },
    { operator: 'null', label: 'Is null', requiresValue: false },
    { operator: 'not_null', label: 'Is not null', requiresValue: false },
  ],
  int: [
    { operator: 'eq', label: 'Equals', requiresValue: true },
    { operator: 'neq', label: 'Not equals', requiresValue: true },
    { operator: 'gt', label: 'Greater than', requiresValue: true },
    { operator: 'gte', label: 'Greater or equal', requiresValue: true },
    { operator: 'lt', label: 'Less than', requiresValue: true },
    { operator: 'lte', label: 'Less or equal', requiresValue: true },
    { operator: 'in', label: 'In list', requiresValue: true },
    { operator: 'not_in', label: 'Not in list', requiresValue: true },
    { operator: 'null', label: 'Is null', requiresValue: false },
    { operator: 'not_null', label: 'Is not null', requiresValue: false },
  ],
  float: [
    { operator: 'eq', label: 'Equals', requiresValue: true },
    { operator: 'neq', label: 'Not equals', requiresValue: true },
    { operator: 'gt', label: 'Greater than', requiresValue: true },
    { operator: 'gte', label: 'Greater or equal', requiresValue: true },
    { operator: 'lt', label: 'Less than', requiresValue: true },
    { operator: 'lte', label: 'Less or equal', requiresValue: true },
    { operator: 'in', label: 'In list', requiresValue: true },
    { operator: 'not_in', label: 'Not in list', requiresValue: true },
    { operator: 'null', label: 'Is null', requiresValue: false },
    { operator: 'not_null', label: 'Is not null', requiresValue: false },
  ],
  datetime: [
    { operator: 'eq', label: 'Equals', requiresValue: true },
    { operator: 'neq', label: 'Not equals', requiresValue: true },
    { operator: 'gt', label: 'After', requiresValue: true },
    { operator: 'gte', label: 'After or equal', requiresValue: true },
    { operator: 'lt', label: 'Before', requiresValue: true },
    { operator: 'lte', label: 'Before or equal', requiresValue: true },
    { operator: 'null', label: 'Is null', requiresValue: false },
    { operator: 'not_null', label: 'Is not null', requiresValue: false },
  ],
  uuid: [
    { operator: 'eq', label: 'Equals', requiresValue: true },
    { operator: 'neq', label: 'Not equals', requiresValue: true },
    { operator: 'in', label: 'In list', requiresValue: true },
    { operator: 'not_in', label: 'Not in list', requiresValue: true },
    { operator: 'null', label: 'Is null', requiresValue: false },
    { operator: 'not_null', label: 'Is not null', requiresValue: false },
  ],
  relationship: [
    { operator: 'eq', label: 'Equals', requiresValue: true },
    { operator: 'neq', label: 'Not equals', requiresValue: true },
    { operator: 'in', label: 'In list', requiresValue: true },
    { operator: 'not_in', label: 'Not in list', requiresValue: true },
    { operator: 'null', label: 'Is null', requiresValue: false },
    { operator: 'not_null', label: 'Is not null', requiresValue: false },
  ],
}

/** Check if a condition is a group (has `conditions` property) */
export function isFilterGroup(condition: FilterCondition): condition is FilterGroup {
  return 'conditions' in condition
}

/** Check if a condition is a rule (has `field` property) */
export function isFilterRule(condition: FilterCondition): condition is FilterRule {
  return 'field' in condition
}

/** Create an empty rule with sensible defaults */
export function createEmptyRule(): FilterRule {
  return { field: '', operator: 'eq', value: '' }
}

/** Create an empty group with AND operator */
export function createEmptyGroup(): FilterGroup {
  return { operator: 'and', conditions: [] }
}

// ---------------------------------------------------------------------------
// Short-form JSON conversion (matching the Filter API spec)
// ---------------------------------------------------------------------------

/** Map internal operator names to short-form (with _ prefix) */
const OPERATOR_TO_SHORT: Record<string, string> = {
  eq: '_eq',
  neq: '_neq',
  gt: '_gt',
  gte: '_gte',
  lt: '_lt',
  lte: '_lte',
  contains: '_contains',
  starts_with: '_starts_with',
  ends_with: '_ends_with',
  in: '_in',
  not_in: '_nin',
  null: '_null',
  not_null: '_nnull',
}

/** Map short-form operator names back to internal names */
const SHORT_TO_OPERATOR: Record<string, string> = {
  _eq: 'eq',
  _neq: 'neq',
  _gt: 'gt',
  _gte: 'gte',
  _lt: 'lt',
  _lte: 'lte',
  _contains: 'contains',
  _starts_with: 'starts_with',
  _ends_with: 'ends_with',
  _in: 'in',
  _nin: 'not_in',
  _null: 'null',
  _nnull: 'not_null',
}

/** Convert internal FilterCondition to short-form JSON for the API */
export function toShortForm(cond: FilterCondition): Record<string, unknown> {
  if (isFilterGroup(cond)) {
    const key = cond.operator === 'and' ? '_and' : '_or'
    return { [key]: cond.conditions.map(toShortForm) }
  }
  const rule = cond as FilterRule
  const opKey = OPERATOR_TO_SHORT[rule.operator] || rule.operator
  const val = rule.value !== undefined && rule.value !== null && rule.value !== ''
    ? rule.value
    : null
  return { [rule.field]: { [opKey]: val } }
}

/** Parse short-form JSON back to internal FilterCondition */
export function fromShortForm(json: Record<string, unknown>): FilterCondition | null {
  const keys = Object.keys(json)
  if (keys.length === 0) return null
  const key = keys[0]

  if (key === '_and' || key === '_or') {
    const arr = json[key] as Record<string, unknown>[]
    const group: FilterGroup = {
      operator: key === '_and' ? 'and' : 'or',
      conditions: arr.map(fromShortForm).filter((c): c is FilterCondition => c !== null),
    }
    return group
  }

  // Single rule: { field_name: { _op: value } }
  const inner = json[key] as Record<string, unknown>
  if (!inner || typeof inner !== 'object') return null
  const opKeys = Object.keys(inner)
  if (opKeys.length === 0) return null
  const opKey = opKeys[0]
  const internalOp = SHORT_TO_OPERATOR[opKey] || opKey
  return {
    field: key,
    operator: internalOp as FilterOperator,
    value: inner[opKey],
  } as FilterRule
}
