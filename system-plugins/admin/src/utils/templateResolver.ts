export function resolveTemplate(template: string, item: Record<string, any>): string {
  return template.replace(/\{\{\s*([^}]+)\s*\}\}/g, (_, expr: string) => {
    const path = expr.trim()
    const val = getNestedValue(item, path)
    return val !== null && val !== undefined ? String(val) : ''
  })
}

function getNestedValue(obj: any, path: string): any {
  return path.split('.').reduce((acc, key) => {
    if (acc && typeof acc === 'object' && key in acc) return acc[key]
    return undefined
  }, obj)
}
