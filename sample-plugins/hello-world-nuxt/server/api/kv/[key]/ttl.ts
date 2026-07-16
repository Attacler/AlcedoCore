import { defineEventHandler, getRouterParam } from 'h3'
import { alcedo } from '../../../../composables/useAlcedo'
import { useRequestId } from '../../../../composables/useRequestId'

export default defineEventHandler(async (event) => {
  const key = getRouterParam(event, 'key')!
  const rid = useRequestId(event)
  const ttl = await alcedo.kv.ttl(key, { requestId: rid })
  return { key, ttl }
})
