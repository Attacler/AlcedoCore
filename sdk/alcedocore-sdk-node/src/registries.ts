export function createRegistriesResource(ky: any) {
  return {
    list: (options?: any) => ky.get("platform/registries", options).json(),
    get: (id: number, options?: any) => ky.get(`platform/registries/${id}`, options).json(),
    create: (data: any, options?: any) => ky.post("platform/registries", { json: data, ...options }).json(),
    update: (id: number, data: any, options?: any) => ky.put(`platform/registries/${id}`, { json: data, ...options }).json(),
    delete: (id: number, options?: any) => ky.delete(`platform/registries/${id}`, options).json(),
    health_check: (id: number, options?: any) => ky.get(`platform/registries/${id}/health`, options).json(),
    healthCheck: (id: number, options?: any) => ky.get(`platform/registries/${id}/health`, options).json(),
    healthCheckUrl: (url: string, options?: any) =>
      ky.post("platform/registries/health-check", { json: { url }, ...options }).json(),
    images: (id: number, options?: any) => ky.get(`platform/registries/${id}/images`, options).json(),
  };
}
