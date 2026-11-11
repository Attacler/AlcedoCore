// Returns the base path for calling this plugin's own API endpoints.
// When served under a route alias (e.g. /nuxt-test), the core injects
// window.__ALCEDO_BASE__ into the HTML. When served via the canonical
// /p/{install_id} route, we extract the install_id from the URL.
// Falls back to empty string (root-relative) for standalone dev.
export function usePluginApiBase(): string {
  if (typeof window === 'undefined') return ''

  // Injected by the core's HTML rewriter when served under an alias.
  const aliasBase = (window as any).__ALCEDO_BASE__
  if (aliasBase && typeof aliasBase === 'string') {
    return aliasBase
  }

  // Canonical /p/{install_id}/... — extract the install_id.
  const match = window.location.pathname.match(/^\/p\/(\d+)/)
  if (match) {
    return `/p/${match[1]}`
  }

  // Standalone dev (e.g. nuxt dev on :3000) — API is at /api/...
  return ''
}

// Helper to fetch from this plugin's own API.
export async function pluginFetch<T = any>(path: string, opts?: RequestInit): Promise<T> {
  const base = usePluginApiBase()
  const res = await fetch(`${base}/api${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...opts,
  })
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
  return res.json()
}
