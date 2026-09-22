import { DeveloperKey, DeveloperKeyWithRawKey } from "./types/developerKeys";

export function createDeveloperApiKeysResource(ky: any) {
    return {
        /// List developer keys, optionally scoped to a single version.
        list: (versionId?: number, options?: any) =>
            ky
                .get("platform/developer-keys", {
                    ...options,
                    searchParams:
                        versionId != null
                            ? { version_id: String(versionId) }
                            : undefined,
                })
                .json() as Promise<DeveloperKey[]>,
        /// Create a key bound to a version. The raw key is only returned once.
        create: (versionId: number, name: string, options?: any) =>
            ky
                .post("platform/developer-keys", {
                    json: { name, version_id: versionId },
                    ...options,
                })
                .json() as Promise<DeveloperKeyWithRawKey>,
        remove: (id: string, options?: any) =>
            ky
                .delete(
                    `platform/developer-keys/${encodeURIComponent(id)}`,
                    options,
                )
                .json() as Promise<{ success: boolean }>,
    };
}
