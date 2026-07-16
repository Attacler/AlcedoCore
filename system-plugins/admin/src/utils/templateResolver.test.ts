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
})
