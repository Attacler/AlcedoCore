import { describe, it, expect } from 'vitest'
import { resolveTemplate } from './templateResolver'

describe('resolveTemplate', () => {
  const item = {
    number: '42',
    name: 'Acme Corp',
    status: 'active',
    customer: {
      name: 'Alice',
      email: 'alice@example.com',
    },
    nullField: null,
    undefinedField: undefined,
  }

  it('replaces simple {{field}} placeholders', () => {
    expect(resolveTemplate('{{number}} - {{name}}', item)).toBe('42 - Acme Corp')
  })

  it('supports nested paths with dot notation', () => {
    expect(resolveTemplate('{{customer.name}} <{{customer.email}}>', item)).toBe('Alice <alice@example.com>')
  })

  it('handles whitespace inside curly braces', () => {
    expect(resolveTemplate('{{ number }}', item)).toBe('42')
  })

  it('returns empty string for missing fields', () => {
    expect(resolveTemplate('{{missing}}', item)).toBe('')
  })

  it('returns empty string for null/undefined fields', () => {
    expect(resolveTemplate('{{nullField}} - {{undefinedField}}', item)).toBe(' - ')
  })

  it('returns empty string when template has no matches', () => {
    expect(resolveTemplate('plain text', item)).toBe('plain text')
  })

  it('handles mixed content with template and plain text', () => {
    expect(resolveTemplate('Item #{{number}}: {{name}}', item)).toBe('Item #42: Acme Corp')
  })

  it('uses fallback when a field is missing', () => {
    expect(resolveTemplate('{{missing || N/A}}', item)).toBe('N/A')
  })

  it('uses fallback when a field is null/empty', () => {
    expect(resolveTemplate('{{nullField || none}}', item)).toBe('none')
  })

  it('does not use fallback when the field has a value', () => {
    expect(resolveTemplate('{{name || fallback}}', item)).toBe('Acme Corp')
  })

  it('formats datetime fields as short dates', () => {
    const fields = [{ name: 'created_at', type: 'datetime' }]
    const withDate = { ...item, created_at: '2026-08-15T10:30:00Z' }
    expect(resolveTemplate('{{created_at}}', withDate, fields)).toBe(
      new Date('2026-08-15T10:30:00Z').toLocaleDateString(),
    )
  })

  it('formats boolean fields as Yes/No', () => {
    const fields = [{ name: 'is_active', type: 'boolean' }]
    expect(resolveTemplate('{{is_active}}', { ...item, is_active: true }, fields)).toBe('Yes')
    expect(resolveTemplate('{{is_active}}', { ...item, is_active: false }, fields)).toBe('No')
  })

  it('prefers the display value for relationship fields', () => {
    const fields = [{ name: 'customer_id', type: 'relationship' }]
    const withRel = { ...item, customer_id: 'abc-123', customer_id__display_value: 'Alice' }
    expect(resolveTemplate('{{customer_id}}', withRel, fields)).toBe('Alice')
  })

  it('renders the raw value when no fields are provided', () => {
    expect(resolveTemplate('{{name}}', item)).toBe('Acme Corp')
  })
})
