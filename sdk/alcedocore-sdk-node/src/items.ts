export function createItemsResource(ky: any) {
  return {
    list: (name: string, params?: Record<string, string>) =>
      ky.get(`items/${encodeURIComponent(name)}`, { searchParams: params }).json(),

    create: (name: string, data: any) =>
      ky.post(`items/${encodeURIComponent(name)}`, { json: data }).json(),

    update: (name: string, data: { filter: any; update: any }) =>
      ky.put(`items/${encodeURIComponent(name)}`, { json: data }).json(),

    delete: (name: string, data: { filter?: any; pk_values?: any[] }) =>
      ky.delete(`items/${encodeURIComponent(name)}`, { json: data }).json(),

    get: (name: string, id: string, params?: Record<string, string>) =>
      ky.get(`items/${encodeURIComponent(name)}/${encodeURIComponent(id)}`, { searchParams: params }).json(),

    patch: (name: string, id: string, data: any) =>
      ky.patch(`items/${encodeURIComponent(name)}/${encodeURIComponent(id)}`, { json: data }).json(),

    query: (name: string, data: any) => {
      const params = new URLSearchParams()
      if (data.filter) params.set('filter', JSON.stringify(data.filter))
      if (data.limit) params.set('limit', String(data.limit))
      if (data.offset) params.set('offset', String(data.offset))
      if (data.fields) params.set('fields', data.fields.join(','))
      if (data.sort && data.sort.length > 0) {
        params.set('sort', data.sort[0].field)
        if (data.sort[0].order || data.sort[0].direction) {
          params.set('order', data.sort[0].order || data.sort[0].direction)
        }
      }
      return ky.get(`items/${encodeURIComponent(name)}`, { searchParams: params }).json()
    },

    grouped: (name: string, data: any) =>
      ky.post(`items/${encodeURIComponent(name)}/grouped`, { json: data }).json(),

    references: (name: string, id: string) =>
      ky.get(`items/${encodeURIComponent(name)}/${encodeURIComponent(id)}/references`).json(),
  };
}
