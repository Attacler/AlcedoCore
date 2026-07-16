import { PluginPagesResponseSchema, PluginAssetsResponseSchema } from "./zod-schemas.js";

export function createPluginsResource(ky: any) {
  return {
    list: (options?: any) => ky.get("plugins", options).json(),
    get: (name: string, options?: any) => ky.get(`plugins/${name}`, options).json(),
    schema: (name: string, options?: any) => ky.get(`plugins/${name}/schema`, options).json(),
    declarations: (name: string, options?: any) => ky.get(`plugins/${name}/declarations`, options).json(),
    pages: (name: string, options?: any) =>
      ky.get(`plugins/${name}/pages`, options).json().then(
        (res: any) => PluginPagesResponseSchema.parse(res).pages,
      ),
    assets: (name: string, options?: any) =>
      ky.get(`plugins/${name}/pages/assets`, options).json().then(
        (res: any) => PluginAssetsResponseSchema.parse(res),
      ),
    install: (zipFile: File | Blob, options?: any) => {
      const formData = new FormData();
      formData.append("zip", zipFile);
      return ky.post("plugins/install", { body: formData, ...options }).json();
    },
    uninstall: (name: string, options?: any) =>
      ky.delete("plugins/uninstall", { json: { name }, ...options }).json(),
    update: (name: string, zipFile: File | Blob, options?: any) => {
      const formData = new FormData();
      formData.append("zip", zipFile);
      return ky.put(`plugins/${name}`, { body: formData, ...options }).json();
    },
    enable: (name: string, options?: any) =>
      ky.post(`plugins/${encodeURIComponent(name)}/enable`, options).json(),
    disable: (name: string, options?: any) =>
      ky.post(`plugins/${encodeURIComponent(name)}/disable`, options).json(),
    create: (data: any, options?: any) =>
      ky.post("plugins", { json: data, ...options }).json(),
    createFromRegistry: (data: { slug: string; image: string; status?: string }, options?: any) =>
      ky.post("plugins", { json: data, ...options }).json(),
    delete: (name: string, options?: any) =>
      ky.delete(`plugins/${encodeURIComponent(name)}`, options).json(),
    docs: (name: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(name)}/docs`, options).json(),
    docContent: (name: string, path: string, options?: any) => {
      let cleanPath = path;
      if (path.startsWith("docs/")) cleanPath = path.slice(5);
      else if (path.startsWith("./")) cleanPath = path.slice(2);
      return ky.get(`plugins/${encodeURIComponent(name)}/docs/${cleanPath}`, options).text();
    },
    docker: (name: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(name)}/docker`, options).json(),
    versions: (name: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(name)}/versions`, options).json(),
    deploy: (name: string, tag: string, options?: any) =>
      ky.post(`plugins/${encodeURIComponent(name)}/deploy`, { json: { tag }, ...options }).json(),
    instances: (slug: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(slug)}/instances`, options).json(),
    instance: (slug: string, taskId: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(slug)}/instances/${encodeURIComponent(taskId)}`, options).json(),
    instanceStats: (slug: string, taskId: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(slug)}/instances/${encodeURIComponent(taskId)}/stats`, options).json(),
    instanceLogs: (slug: string, taskId: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(slug)}/instances/${encodeURIComponent(taskId)}/logs`, options).json(),
    scale: (slug: string, data: { replicas: number; resource_limits?: { cpu_limit: number; memory_limit: number } }, options?: any) =>
      ky.post(`plugins/${encodeURIComponent(slug)}/scale`, { json: data, ...options }),
    restart: (name: string, containerId?: string, options?: any) =>
      ky.post(`plugins/${encodeURIComponent(name)}/restart`, { json: { container_id: containerId }, ...options }),
    scopes: (slug: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(slug)}/scopes`, options).json(),
    updateScopes: (slug: string, scopes: string[], options?: any) =>
      ky.post(`plugins/${encodeURIComponent(slug)}/scopes`, { json: { scopes }, ...options }),
    requestLogs: (slug: string, params?: URLSearchParams, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(slug)}/logs`, { searchParams: params as any, ...options }).json(),
    requestLogDetail: (slug: string, requestId: string, options?: any) =>
      ky.get(`plugins/${encodeURIComponent(slug)}/logs/${encodeURIComponent(requestId)}`, options).json(),
  };
}
