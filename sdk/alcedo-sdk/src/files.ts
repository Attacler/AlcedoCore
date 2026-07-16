export function createFilesResource(ky: any) {
  return {
    upload: (file: File | Blob, filename: string, options?: {
      collection_name?: string
      item_id?: string
      field_name?: string
    }) => {
      const formData = new FormData()
      formData.append('file', file, filename)
      if (options?.collection_name) formData.append('collection_name', options.collection_name)
      if (options?.item_id) formData.append('item_id', options.item_id)
      if (options?.field_name) formData.append('field_name', options.field_name)
      return ky.post('files/upload', { body: formData }).json()
    },

    download: (id: string) =>
      ky.get(`files/${id}/download`),

    get: (id: string) =>
      ky.get(`files/${id}`).json(),

    list: (params?: { search?: string; mime_type?: string; limit?: number; offset?: number }) =>
      ky.get('files', { searchParams: params as Record<string, string> }).json(),

    update: (id: string, data: { alt_text?: string; filename?: string }) =>
      ky.patch(`files/${id}`, { json: data }).json(),

    delete: (id: string) =>
      ky.delete(`files/${id}`).json(),

    batchDelete: (ids: string[]) =>
      ky.post('files/batch/delete', { json: { ids } }).json(),
  }
}
