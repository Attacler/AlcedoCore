export const EVENT_TYPES = [
  { label: 'Item Created', value: 'ItemCreated' },
  { label: 'Item Updated', value: 'ItemUpdated' },
  { label: 'Item Deleted', value: 'ItemDeleted' },
]

export function getQueryParam(name: string): string | null {
  const params = new URLSearchParams(window.location.hash.split('?')[1] || '')
  return params.get(name)
}
