import { ListFilesParameters, MediaFile } from "./types/files";
import type { ClientOptions } from "./client";

export interface DownloadUrlOptions {
    app?: string;
    version?: string;
}

/** Anything that carries a server-built `download_url` or a bare file id. */
export type FileRef =
    | string
    | ({ download_url?: string | null; id?: string } | null | undefined);

/**
 * Builds the `<img src>` / download path for a file.
 *
 * Prefers the server's `download_url` when present (it already carries the
 * owning app/version, so cross-app relational images work); otherwise builds
 * the same path from the id + app/version.
 */
export function resolveDownloadUrl(
    baseUrl: string,
    defaults: DownloadUrlOptions | undefined,
    file: FileRef,
    opts?: DownloadUrlOptions,
): string {
    if (file && typeof file === "object" && file.download_url) {
        return file.download_url;
    }
    const id = typeof file === "string" ? file : file?.id;
    if (!id) return "";

    const app = opts?.app ?? defaults?.app;
    const version = opts?.version ?? defaults?.version;
    const params = new URLSearchParams();
    if (app) params.set("ac_app", app);
    if (version) params.set("ac_version", version);
    const query = params.toString();

    const root = baseUrl.replace(/\/$/, "");
    return `${root}/api/app/files/${encodeURIComponent(id)}/download${
        query ? `?${query}` : ""
    }`;
}

export function createFilesResource(
    ky: any,
    baseUrl = "",
    defaults?: ClientOptions,
) {
    return {
        upload: (
            file: File | Blob,
            filename: string,
            options?: {
                collection_name?: string;
                item_id?: string;
                field_name?: string;
                folder_id?: string;
                overwrite?: boolean;
            },
        ) => {
            const formData = new FormData();
            formData.append("file", file, filename);
            if (options?.collection_name)
                formData.append("collection_name", options.collection_name);
            if (options?.item_id) formData.append("item_id", options.item_id);
            if (options?.field_name)
                formData.append("field_name", options.field_name);
            if (options?.folder_id) formData.append("folder_id", options.folder_id);
            if (options?.overwrite) formData.append("overwrite", "true");
            return ky.post("app/files/upload", { body: formData }).json();
        },

        /**
         * The URL to use as an `<img src>` (or a download link) for a file —
         * pass the metadata object if you have it, or just its id.
         */
        downloadUrl: (file: FileRef, opts?: DownloadUrlOptions) =>
            resolveDownloadUrl(
                baseUrl,
                { app: defaults?.app, version: defaults?.version },
                file,
                opts,
            ),

        download: (id: string) => ky.get(`app/files/${id}/download`),

        get: (id: string) => ky.get(`app/files/${id}`).json() as MediaFile,

        list: (params?: ListFilesParameters) =>
            ky
                .get("app/files", {
                    searchParams: params as Record<string, string>,
                })
                .json() as {
                data: MediaFile[];
                total: number;
            },

        update: (
            id: string,
            data: { alt_text?: string; filename?: string; folder_id?: string },
        ) => ky.patch(`app/files/${id}`, { json: data }).json(),

        delete: (id: string) => ky.delete(`app/files/${id}`).json(),

        batchDelete: (ids: string[]) =>
            ky.post("app/files/batch/delete", { json: { ids } }).json(),

        folders: {
            create: (name: string, parent_id?: string) =>
                ky.post("app/files/folders", { json: { name, parent_id } }).json(),

            list: (parent_id?: string) => {
                const params: Record<string, string> = {};
                if (parent_id) params.parent_id = parent_id;
                return ky
                    .get("app/files/folders", { searchParams: params })
                    .json();
            },

            get: (id: string) => ky.get(`app/files/folders/${id}`).json(),

            update: (
                id: string,
                data: { name?: string; parent_id?: string | null },
            ) => ky.patch(`app/files/folders/${id}`, { json: data }).json(),

            delete: (id: string, recursive?: boolean) =>
                ky
                    .delete(`app/files/folders/${id}`, {
                        searchParams: recursive
                            ? { recursive: "true" }
                            : undefined,
                    })
                    .json(),
        },
    };
}
