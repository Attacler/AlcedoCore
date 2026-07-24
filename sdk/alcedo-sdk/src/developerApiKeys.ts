import { DeveloperKey, DeveloperKeyWithRawKey } from "./types/developerKeys";

export function createDeveloperApiKeysResource(ky: any) {
    return {
        list: (options?: any) =>
            ky
                .get("/settings/developer/keys", options)
                .json() as DeveloperKey[],
        create: (name: string, options?: any) =>
            ky
                .post("/settings/developer/keys", {
                    json: { name },
                    ...options,
                })
                .json() as DeveloperKeyWithRawKey,
        remove: (id: string, options?: any) =>
            ky
                .delete(
                    `/settings/developer/keys/${encodeURIComponent(id)}`,
                    options,
                )
                .json() as { success: boolean },
    };
}
