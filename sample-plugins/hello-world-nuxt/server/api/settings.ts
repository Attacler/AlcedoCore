import { defineEventHandler } from 'h3'
import { alcedo } from '../../composables/useAlcedo'
import { useRequestId } from '../../composables/useRequestId'

export default defineEventHandler(async (event) => {
  const rid = useRequestId(event)
  // Read plugin settings from the KV store (key: "settings").
  // Falls back to defaults from the manifest settings_schema.
  const defaults = { greeting: 'Hello from Nuxt plugin!' }
  try {
    const stored = await alcedo.kv.get('hello-world-nuxt:settings', { requestId: rid })
    return { settings: { ...defaults, ...(stored ?? {}) } }
  } catch {
    return { settings: defaults }
  }
})
