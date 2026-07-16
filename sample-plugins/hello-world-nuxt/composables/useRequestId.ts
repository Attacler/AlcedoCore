import { getHeader } from 'h3'

// Extract X-Request-ID from the incoming h3 event
// so API routes can pass it to the SDK for auth back to the core
export function useRequestId(event: H3Event): string | undefined {
  return getHeader(event, 'x-request-id') ?? undefined
}
