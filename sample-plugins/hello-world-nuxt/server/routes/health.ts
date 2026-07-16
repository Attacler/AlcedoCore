import { defineEventHandler } from 'h3'
import { useRequestId } from '../../composables/useRequestId'

const CORE_URL = process.env.CORE_URL || 'http://core:8080'

export default defineEventHandler(async (event) => {
  const rid = useRequestId(event)
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (rid) {
    headers['X-Request-ID'] = rid
  }
  const res = await fetch(`${CORE_URL}/health`, { headers })
  const body = await res.json()
  return { status: 'healthy', service: 'hello-world-nuxt', core: body }
})
