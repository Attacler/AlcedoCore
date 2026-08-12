import { ref, type Ref } from 'vue'

export interface PendingOp {
  type: 'create' | 'update' | 'delete'
  tempId?: string
  id?: string
  childCollection: string
  fkFieldName?: string
  values?: Record<string, any>
  nestedOps?: PendingOp[]
}

/** A leaf create is one with no nested (grandchild) ops queued under it. */
export function isLeafCreate(op: PendingOp): boolean {
  return op.type === 'create' && (!op.nestedOps || op.nestedOps.length === 0)
}

/** Serialize queued ops so an outer form can execute them after the parent exists. */
export function serializePendingOps(
  pendingOps: PendingOp[],
  childCollectionName: string,
  fkFieldName?: string,
): PendingOp[] {
  return pendingOps.map((op) => ({
    ...op,
    childCollection: op.childCollection || childCollectionName,
    fkFieldName: op.fkFieldName ?? fkFieldName,
    values: { ...(op.values || {}) },
    nestedOps: op.nestedOps ? op.nestedOps.map((n) => ({ ...n })) : [],
  }))
}

export interface CreateBodyResult {
  body: Record<string, any> | null
  inlinedTempIds: string[]
}

/** Build the nested O2M create body for a single parent POST.
 *
 * Returns `{ body, inlinedTempIds }` where `body` is
 * `{ [childCollection]: { create: [...] } }` for queued *leaf* creates only
 * (no nested ops), and `inlinedTempIds` lists the ops inlined so the caller
 * can drop them after the request succeeds. Pure — never mutates the queue.
 * Creates with nested ops stay queued and are flushed after the parent create
 * via `flushPending`. The FK is left for the backend to set from the parent id. */
export function buildCreateBody(
  pendingOps: PendingOp[],
  childCollectionName: string,
  fkFieldName?: string,
): CreateBodyResult {
  const creates = pendingOps.filter(isLeafCreate)
  if (creates.length === 0) return { body: null, inlinedTempIds: [] }
  const createObjs = creates.map((op) => {
    const vals = { ...(op.values || {}) }
    if (fkFieldName) delete vals[fkFieldName]
    return vals
  })
  return {
    body: { [childCollectionName]: { create: createObjs } },
    inlinedTempIds: creates.map((op) => op.tempId!).filter(Boolean),
  }
}

/** Create a reactive pending-op queue with the serialization helpers. */
export function useRelationalQueue() {
  const pendingOps = ref<PendingOp[]>([])

  function collect(childCollectionName: string, fkFieldName?: string): PendingOp[] {
    return serializePendingOps(pendingOps.value, childCollectionName, fkFieldName)
  }

  function getCreateBody(childCollectionName: string, fkFieldName?: string): CreateBodyResult {
    return buildCreateBody(pendingOps.value, childCollectionName, fkFieldName)
  }

  function consumeInlinedCreates(tempIds: string[]): void {
    if (!tempIds.length) return
    pendingOps.value = pendingOps.value.filter(
      (op) => !(op.type === 'create' && tempIds.includes(op.tempId!)),
    )
  }

  function clear(): void {
    pendingOps.value = []
  }

  function push(op: PendingOp): void {
    pendingOps.value.push(op)
  }

  function replaceByTempId(tempId: string, values: Record<string, any>): void {
    const op = pendingOps.value.find((o) => o.tempId === tempId)
    if (op) op.values = values
  }

  function removeByTempId(tempId: string): void {
    pendingOps.value = pendingOps.value.filter((op) => op.tempId !== tempId)
  }

  return { pendingOps, collect, getCreateBody, consumeInlinedCreates, clear, push, replaceByTempId, removeByTempId }
}

export type RelationalQueue = ReturnType<typeof useRelationalQueue>
export type { Ref }
