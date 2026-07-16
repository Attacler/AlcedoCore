import type { PluginSchemaResponse } from 'alcedo-sdk'

export function schemaToMermaid(schema: PluginSchemaResponse): string {
  if (!schema.tables || schema.tables.length === 0) {
    return ''
  }

  const lines: string[] = ['erDiagram']

  for (const table of schema.tables) {
    lines.push(`    ${table.name} {`)
    for (const col of table.columns) {
      const parts: string[] = [col.type, col.name]

      if (table.primaryKey && table.primaryKey.columns.includes(col.name)) {
        parts.push('PK')
      }

      if (col.nullable) {
        parts.push(`"nullable"`)
      }

      lines.push(`        ${parts.join(' ')}`)
    }
    lines.push('    }')
  }

  for (const table of schema.tables) {
    for (const fk of table.foreignKeys) {
      const sourceTable = table.name
      const targetTable = fk.foreignTableName
      lines.push(`    ${targetTable} ||--o{ ${sourceTable} : "${fk.columnName}"`)
    }
  }

  return lines.join('\n')
}
