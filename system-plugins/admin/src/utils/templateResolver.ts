export function resolveTemplate(
  template: string,
  item: Record<string, any>,
  fields?: { name: string; type?: string }[],
): string {
  return template.replace(/\{\{\s*([^}]+?)\s*\}\}/g, (_, expr: string) => {
    const [pathRaw, fallbackRaw] = expr.split('||')
    const path = pathRaw.trim()
    const fallback = fallbackRaw ? fallbackRaw.trim() : ''
    const field = fields?.find((f) => f.name === path)

    let val = getNestedValue(item, path)
    if (val !== null && val !== undefined && field?.type === 'relationship') {
      const display = getNestedValue(item, path + '__display_value')
      if (display !== null && display !== undefined) val = display
    }

    if (val === null || val === undefined || val === '') {
      return fallback
    }
    return formatValue(val, field?.type)
  })
}

function formatValue(value: any, type?: string): string {
  if (type === 'datetime') {
    const d = new Date(value)
    if (!isNaN(d.getTime())) return d.toLocaleDateString()
  }
  if (type === 'boolean') return value ? 'Yes' : 'No'
  return String(value)
}

function getNestedValue(obj: any, path: string): any {
  return path.split('.').reduce((acc, key) => {
    if (acc && typeof acc === 'object' && key in acc) return acc[key]
    return undefined
  }, obj)
}
