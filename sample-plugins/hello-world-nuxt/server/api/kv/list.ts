import { defineEventHandler, getQuery } from 'h3'
import { alcedo } from '../../../composables/useAlcedo'
import { useRequestId } from '../../../composables/useRequestId'

export default defineEventHandler(async (event) => {
  const query = getQuery(event)
  const prefix = (query.prefix as string) || undefined
  const rid = useRequestId(event)
  const entries = await alcedo.kv.list(prefix, { requestId: rid })
  return { prefix: prefix || '', entries }
})
