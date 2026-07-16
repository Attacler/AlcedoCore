let initialized = false

export async function initMermaid() {
  if (initialized) {
    return import('mermaid')
  }

  const mermaid = await import('mermaid')

  mermaid.default.initialize({
    theme: 'default',
    startOnLoad: false,
  })

  initialized = true

  return mermaid
}
