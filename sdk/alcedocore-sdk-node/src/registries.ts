export function createRegistriesResource(ky: any) {
  return {
    list: (options?: any) => ky.get("registries", options).json(),
    get: (id: number, options?: any) => ky.get(`registries/${id}`, options).json(),
    create: (data: any, options?: any) => ky.post("registries", { json: data, ...options }).json(),
    update: (id: number, data: any, options?: any) => ky.put(`registries/${id}`, { json: data, ...options }).json(),
    delete: (id: number, options?: any) => ky.delete(`registries/${id}`, options).json(),
    health_check: (id: number, options?: any) => ky.get(`registries/${id}/health`, options).json(),
    healthCheck: (id: number, options?: any) => ky.get(`registries/${id}/health`, options).json(),
    images: (id: number, options?: any) => ky.get(`registries/${id}/images`, options).json(),
  };
}
