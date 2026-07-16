import { defineEventHandler, getRouterParam, readBody, createError } from 'h3'
import { alcedo } from '../../../composables/useAlcedo'
import { useRequestId } from '../../../composables/useRequestId'

export default defineEventHandler(async (event) => {
  const key = getRouterParam(event, 'key')!
  const rid = useRequestId(event)

  try {
    if (event.method === 'GET') {
      const result = await alcedo.kv.get(key, { requestId: rid })
      return { key, value: result }
    }

    if (event.method === 'PUT') {
      const body = await readBody<{ value: any; ttl?: number }>(event)
      const result = await alcedo.kv.set(key, body?.value, body?.ttl, { requestId: rid })
      return { key, value: body?.value, ttl: body?.ttl, result }
    }

    if (event.method === 'DELETE') {
      const result = await alcedo.kv.delete(key, { requestId: rid })
      return { key, deleted: true }
    }

    throw createError({ statusCode: 405, statusMessage: 'Method not allowed' })
  } catch (err: any) {
    // ky's HTTPError carries the response status and body
    const status = err.response?.status || err.statusCode || 500
    const body = err.response ? await err.response.json().catch(() => ({})) : {}
    throw createError({
      statusCode: status,
      statusMessage: body.error || body.message || err.message || 'Internal Server Error',
    })
  }
})
