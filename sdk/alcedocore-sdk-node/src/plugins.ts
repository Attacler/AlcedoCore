import {
    PluginPagesResponseSchema,
    PluginAssetsResponseSchema,
} from "./zod-schemas.js";

const path = (suffix: string) => `platform/plugins/${suffix}`;
const slugPath = (slug: string, suffix = "") =>
    path(`${encodeURIComponent(slug)}${suffix}`);
const installPath = (slug: string, appVersionId: number, suffix = "") =>
    slugPath(slug, `/installs/${appVersionId}${suffix}`);
// Runtime/instances are keyed by the install's own id, not by slug + version.
const installIdPath = (installId: number, suffix = "") =>
    path(`installs/${encodeURIComponent(String(installId))}${suffix}`);

export function createPluginsResource(ky: any) {
    return {
        list: (options?: any) => ky.get("platform/plugins", options).json(),
        get: (slug: string, options?: any) =>
            ky.get(slugPath(slug), options).json(),
        update: (slug: string, data: any, options?: any) =>
            ky.put(slugPath(slug), { json: data, ...options }).json(),
        delete: (slug: string, options?: any) =>
            ky.delete(slugPath(slug), options).json(),

        // --- deploy ---
        deploy: (data: any, options?: any) =>
            ky.post(path("deploy"), { json: data, ...options }).json(),
        preview: (image: string, registryId?: number, options?: any) =>
            ky
                .post(path("preview"), {
                    json: { image, registry_id: registryId },
                    ...options,
                })
                .json(),

        // --- installs (plugin × version) ---
        uninstall: (slug: string, appVersionId: number, options?: any) =>
            ky.delete(installPath(slug, appVersionId), options).json(),
        enable: (slug: string, appVersionId: number, options?: any) =>
            ky.post(installPath(slug, appVersionId, "/enable"), options).json(),
        disable: (slug: string, appVersionId: number, options?: any) =>
            ky
                .post(installPath(slug, appVersionId, "/disable"), options)
                .json(),
        getSettings: (slug: string, appVersionId: number, options?: any) =>
            ky
                .get(installPath(slug, appVersionId, "/settings"), options)
                .json(),
        updateSettings: (
            slug: string,
            appVersionId: number,
            data: any,
            options?: any,
        ) =>
            ky
                .patch(installPath(slug, appVersionId, "/settings"), {
                    json: data,
                    ...options,
                })
                .json(),
        getScopes: (slug: string, appVersionId: number, options?: any) =>
            ky.get(installPath(slug, appVersionId, "/scopes"), options).json(),
        updateScopes: (
            slug: string,
            appVersionId: number,
            scopes: string[],
            options?: any,
        ) =>
            ky
                .post(installPath(slug, appVersionId, "/scopes"), {
                    json: { scopes },
                    ...options,
                })
                .json(),

        // Runtime detail and instances are keyed by install id, not slug: a slug
        // can be installed on several app versions and each install is its own
        // deployment, so the slug-keyed shape can no longer name one.
        runtimeInfo: (installId: number, options?: any) =>
            ky.get(installIdPath(installId, "/runtime"), options).json(),
        instances: (installId: number, options?: any) =>
            ky.get(installIdPath(installId, "/instances"), options).json(),
        versions: (slug: string, options?: any) =>
            ky.get(slugPath(slug, "/versions"), options).json(),
        requestLogs: (slug: string, params?: URLSearchParams, options?: any) =>
            ky
                .get(slugPath(slug, "/logs"), {
                    searchParams: params as any,
                    ...options,
                })
                .json(),
        docs: (slug: string, options?: any) =>
            ky.get(slugPath(slug, "/docs"), options).json(),
        docContent: (slug: string, docPath: string, options?: any) => {
            let clean = docPath;
            if (clean.startsWith("docs/")) clean = clean.slice(5);
            else if (clean.startsWith("./")) clean = clean.slice(2);
            return ky.get(slugPath(slug, `/docs/${clean}`), options).text();
        },
        schema: (slug: string, options?: any) =>
            ky.get(slugPath(slug, "/schema"), options).json(),
        pages: (slug: string, options?: any) =>
            ky
                .get(slugPath(slug, "/pages"), options)
                .json()
                .then((res: any) => PluginPagesResponseSchema.parse(res).pages),
        assets: (slug: string, options?: any) =>
            ky
                .get(slugPath(slug, "/pages/assets"), options)
                .json()
                .then((res: any) => PluginAssetsResponseSchema.parse(res)),
        scale: (slug: string, data: any, options?: any) =>
            ky.post(slugPath(slug, "/scale"), { json: data, ...options }),
        restart: (slug: string, containerId?: string, options?: any) =>
            ky.post(slugPath(slug, "/restart"), {
                json: { deployment_id: containerId },
                ...options,
            }),
    };
}
