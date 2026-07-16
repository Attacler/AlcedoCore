import { ref, readonly } from 'vue';
import ky from 'ky';

const client = {
  plugins: {
    list: () => ky.get('/api/plugins').then((r: any) => r.json()),
    get: (name: string) => ky.get(`/api/plugins/${name}`).then((r: any) => r.json()),
    enable: (name: string) => ky.post(`/api/plugins/enable`, { json: { name } }).then((r: any) => r.json()),
    disable: (name: string) => ky.post(`/api/plugins/disable`, { json: { name } }).then((r: any) => r.json()),
    install: (zip: any) => {
      const form = new FormData();
      form.append('zip', zip);
      return ky.post('/api/plugins/install', { body: form }).then((r: any) => r.json());
    },
    uninstall: (name: string) => ky.delete(`/api/plugins/uninstall`, { json: { name } }).then((r: any) => r.json()),
    pages: (name: string) => ky.get(`/api/plugins/${name}/pages`).then((r: any) => r.json()).then((res: any) => res.pages),
    assets: (name: string) => ky.get(`/api/plugins/${name}/pages/assets`).then((r: any) => r.json()),
    schema: (name: string) => ky.get(`/api/plugins/${name}/schema`).then((r: any) => r.json()),
    docs: (name: string) => ky.get(`/api/plugins/${name}/docs`).then((r: any) => r.json()),
    docContent: (name: string, path: string) => {
      // Handle various path formats: docs/foo.md, ./foo.md, foo.md
      let cleanPath = path
      if (path.startsWith('docs/')) {
        cleanPath = path.slice(5) // Remove 'docs/' prefix
      } else if (path.startsWith('./')) {
        cleanPath = path.slice(2) // Remove './' prefix
      }
      return ky.get(`/api/plugins/${name}/docs/${cleanPath}`).then((r: any) => r.text())
    },
    docker: (name: string) => ky.get(`/api/plugins/${name}/docker`).then((r: any) => r.json()),
  },
  settings: {
    get: (name: string) => ky.get(`/api/plugins/${name}/settings`).then((r: any) => r.json()),
    update: (name: string, settings: any) => ky.patch(`/api/plugins/${name}/settings`, { json: settings }).then((r: any) => r.json()),
  },
  migrations: {
    list: (name: string) => ky.get(`/api/plugins/${name}/migrations`).then((r: any) => r.json()),
  },
  logs: {
    get: (name: string, params: URLSearchParams) => ky.get(`/api/plugins/${name}/logs?${params}`).then((r: any) => r.json()),
    detail: (name: string, requestId: string) => ky.get(`/api/plugins/${name}/logs/${requestId}`).then((r: any) => r.json()),
  },
};

const isConnected = ref(true);

export function useAlcedoClient(devServerUrl?: string) {
  // Dev mode: provide asset endpoints pointing to dev server
  const assets = devServerUrl
    ? {
        devMode: (name: string) => ({
          js: `${devServerUrl}/dev/js?plugin=${encodeURIComponent(name)}`,
          css: `${devServerUrl}/dev/css?plugin=${encodeURIComponent(name)}`,
        }),
      }
    : null;

  return {
    client,
    isConnected: readonly(isConnected),
    assets,
  };
}