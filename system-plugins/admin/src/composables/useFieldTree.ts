import type { TreeNode } from 'primevue/treenode'
import type { FieldDefinition } from '@/stores/collections'
import type { FilterCondition } from '@/types/filters'

interface BuildTreeOptions {
  fields: FieldDefinition[]
  collectionName: string
  chain?: string[]
  prefix?: string
}

const OPERATOR_MAP: Record<string, string> = {
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

export function convertFilter(filter: FilterCondition | null): Record<string, unknown> | undefined {
  if (!filter) return undefined

  if ('field' in filter && filter.field) {
    const operatorKey = OPERATOR_MAP[filter.operator] || `_${filter.operator}`
    const parts = filter.field.split('.')
    if (parts.length === 1) {
      return { [parts[0]]: { [operatorKey]: filter.value } }
    }
    let result: Record<string, unknown> = { [operatorKey]: filter.value }
    for (let i = parts.length - 1; i >= 0; i--) {
      result = { [parts[i]]: result }
    }
    return result
  }

  if ('conditions' in filter && Array.isArray(filter.conditions)) {
    const logicKey = filter.operator === 'or' ? '_or' : '_and'
    const conditions = filter.conditions
      .map(c => convertFilter(c))
      .filter(Boolean) as Record<string, unknown>[]
    if (conditions.length === 0) return undefined
    return { [logicKey]: conditions }
  }

  return undefined
}

export function buildFieldTree({ fields, collectionName, chain = [], prefix = '' }: BuildTreeOptions): TreeNode[] {
  const nodes: TreeNode[] = []

  for (const field of fields) {
    const key = prefix ? `${prefix}.${field.name}` : field.name

    if (field.type === 'relationship') {
      const relName = field.related_collection
      // Block only immediate self-references (e.g., a category having a parent of the same type).
      // Allow navigating back through already-visited collections for multi-hop paths
      // like orders → customer → contacts → customer → name.
      const isSelfRef = relName === collectionName

      const node: TreeNode = {
        key,
        label: (field.display_name || field.name) + (relName ? ` ▸ ${relName}` : ''),
        leaf: isSelfRef,
        selectable: false,
        data: { relName, chain: [...chain, collectionName] },
      }

      if (!isSelfRef) {
        node.children = []
      }

      nodes.push(node)
    } else {
      nodes.push({
        key,
        label: field.display_name || field.name,
        leaf: true,
        selectable: true,
      })
    }
  }

  return nodes
}
