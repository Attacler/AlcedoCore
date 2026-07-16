/**
 * TypeScript types and validation utilities for dynamic form rendering based on JSON Schema.
 */

export interface JsonSchemaProperty {
  type: 'string' | 'integer' | 'boolean' | 'array' | 'object' | 'number'
  default?: unknown
  secret?: boolean
  enum?: string[]
  description?: string
  minimum?: number
  maximum?: number
  items?: JsonSchemaProperty
}

export interface JsonSchema {
  type: 'object'
  properties: Record<string, JsonSchemaProperty>
  required?: string[]
  $schemaVersion?: number
}

export interface FormValidationError {
  [fieldName: string]: string | undefined
}

/**
 * Validate a single value against a schema property
 */
export function validateValue(value: unknown, property: JsonSchemaProperty): string | undefined {
  if (value === undefined || value === null) {
    return undefined
  }

  switch (property.type) {
    case 'string':
      if (typeof value !== 'string') return `Expected string, got ${typeof value}`
      break
    case 'integer':
      if (!Number.isInteger(value)) return `Expected integer, got ${typeof value}`
      if (property.minimum !== undefined && (value as number) < property.minimum) {
        return `Value must be at least ${property.minimum}`
      }
      if (property.maximum !== undefined && (value as number) > property.maximum) {
        return `Value must be at most ${property.maximum}`
      }
      break
    case 'boolean':
      if (typeof value !== 'boolean') return `Expected boolean, got ${typeof value}`
      break
    case 'array':
      if (!Array.isArray(value)) return `Expected array, got ${typeof value}`
      break
    case 'number':
      if (typeof value !== 'number') return `Expected number, got ${typeof value}`
      break
  }
  return undefined
}

/**
 * Validate all form values against schema
 */
export function validateForm(
  values: Record<string, unknown>,
  schema: JsonSchema
): FormValidationError {
  const errors: FormValidationError = {}

  for (const [fieldName, value] of Object.entries(values)) {
    const property = schema.properties[fieldName]
    if (property) {
      const error = validateValue(value, property)
      if (error) errors[fieldName] = error
    }
  }

  return errors
}

/**
 * Extract initial values from schema defaults
 */
export function getInitialValues(schema: JsonSchema | null): Record<string, unknown> {
  if (!schema?.properties) return {}

  const values: Record<string, unknown> = {}
  for (const [name, prop] of Object.entries(schema.properties)) {
    if (prop.default !== undefined) {
      values[name] = prop.default
    }
  }
  return values
}