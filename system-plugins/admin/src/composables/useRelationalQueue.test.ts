import { describe, it, expect } from 'vitest'
import {
  buildCreateBody,
  serializePendingOps,
  isLeafCreate,
  type PendingOp,
} from './useRelationalQueue'

function op(partial: Partial<PendingOp> & { type: PendingOp['type'] }): PendingOp {
  return {
    childCollection: 'contacts',
    ...partial,
  }
}

describe('buildCreateBody', () => {
  it('inlines leaf creates and strips the FK', () => {
    const ops: PendingOp[] = [
      op({ type: 'create', tempId: '__new__1', values: { first_name: 'Ava', customer: 'parent-1' } }),
      op({ type: 'create', tempId: '__new__2', values: { first_name: 'Leo' } }),
    ]
    const result = buildCreateBody(ops, 'contacts', 'customer')
    expect(result.body).toEqual({
      contacts: {
        create: [
          { first_name: 'Ava' },
          { first_name: 'Leo' },
        ],
      },
    })
    expect(result.inlinedTempIds).toEqual(['__new__1', '__new__2'])
  })

  it('does not inline creates that carry nested ops', () => {
    const ops: PendingOp[] = [
      op({
        type: 'create',
        tempId: '__new__1',
        values: { first_name: 'Ava' },
        nestedOps: [op({ type: 'create', tempId: '__new__1a', values: { name: 'task' } })],
      }),
    ]
    const result = buildCreateBody(ops, 'contacts', 'customer')
    expect(result.body).toBeNull()
    expect(result.inlinedTempIds).toEqual([])
  })

  it('ignores updates and deletes', () => {
    const ops: PendingOp[] = [
      op({ type: 'create', tempId: '__new__1', values: { first_name: 'Ava' } }),
      op({ type: 'update', id: 'real-1', values: { first_name: 'Ava2' } }),
      op({ type: 'delete', id: 'real-2' }),
    ]
    const result = buildCreateBody(ops, 'contacts', 'customer')
    expect(result.inlinedTempIds).toEqual(['__new__1'])
  })

  it('returns null body when there are no leaf creates', () => {
    expect(buildCreateBody([], 'contacts', 'customer').body).toBeNull()
    const nestedOnly: PendingOp[] = [op({
      type: 'create',
      tempId: 'x',
      values: {},
      nestedOps: [],
    })]
    expect(buildCreateBody(nestedOnly, 'contacts', 'customer').inlinedTempIds).toEqual(['x'])
  })

  it('is pure — does not mutate the input queue', () => {
    const ops: PendingOp[] = [
      op({ type: 'create', tempId: '__new__1', values: { first_name: 'Ava', customer: 'p' } }),
    ]
    const before = JSON.stringify(ops)
    buildCreateBody(ops, 'contacts', 'customer')
    expect(JSON.stringify(ops)).toBe(before)
  })
})

describe('serializePendingOps', () => {
  it('defaults childCollection and fkFieldName, deep-copies values', () => {
    const ops: PendingOp[] = [
      op({ type: 'create', tempId: '__new__1', values: { first_name: 'Ava' } }),
    ]
    const out = serializePendingOps(ops, 'contacts', 'customer')
    expect(out[0].childCollection).toBe('contacts')
    expect(out[0].fkFieldName).toBe('customer')
    // mutation of output must not affect the source
    out[0].values!.first_name = 'changed'
    expect(ops[0].values!.first_name).toBe('Ava')
  })
})

describe('isLeafCreate', () => {
  it('true for create without nestedOps', () => {
    expect(isLeafCreate(op({ type: 'create' }))).toBe(true)
  })
  it('false for create with nestedOps', () => {
    expect(isLeafCreate(op({ type: 'create', nestedOps: [op({ type: 'create' })] }))).toBe(false)
  })
  it('false for non-create ops', () => {
    expect(isLeafCreate(op({ type: 'update', id: '1' }))).toBe(false)
    expect(isLeafCreate(op({ type: 'delete', id: '1' }))).toBe(false)
  })
})
