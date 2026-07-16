import { defineEventHandler } from 'h3'
import { alcedo } from '../../composables/useAlcedo'
import { useRequestId } from '../../composables/useRequestId'

export default defineEventHandler(async (event) => {
  const rid = useRequestId(event)
  const data = await alcedo.settings.get('hello-world-nuxt', { requestId: rid })
  return data
})
