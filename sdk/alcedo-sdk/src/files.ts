import { ListFilesParameters, MediaFile } from "./types/files";

export function createFilesResource(ky: any) {
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
            if (options?.folder_id)
                formData.append("folder_id", options.folder_id);
            if (options?.overwrite) formData.append("overwrite", "true");
            return ky.post("files/upload", { body: formData }).json();
        },

        download: (id: string) => ky.get(`files/${id}/download`),

        get: (id: string) => ky.get(`files/${id}`).json() as MediaFile,

        list: (params?: ListFilesParameters) =>
            ky
                .get("files", {
                    searchParams: params as Record<string, string>,
                })
                .json() as {
                data: MediaFile[];
                total: number;
            },

        update: (
            id: string,
            data: { alt_text?: string; filename?: string; folder_id?: string },
        ) => ky.patch(`files/${id}`, { json: data }).json(),

        delete: (id: string) => ky.delete(`files/${id}`).json(),

        batchDelete: (ids: string[]) =>
            ky.post("files/batch/delete", { json: { ids } }).json(),

        folders: {
            create: (name: string, parent_id?: string) =>
                ky.post("files/folders", { json: { name, parent_id } }).json(),

            list: (parent_id?: string) => {
                const params: Record<string, string> = {};
                if (parent_id) params.parent_id = parent_id;
                return ky.get("files/folders", { searchParams: params }).json();
            },

            get: (id: string) => ky.get(`files/folders/${id}`).json(),

            update: (
                id: string,
                data: { name?: string; parent_id?: string | null },
            ) => ky.patch(`files/folders/${id}`, { json: data }).json(),

            delete: (id: string, recursive?: boolean) =>
                ky
                    .delete(`files/folders/${id}`, {
                        searchParams: recursive
                            ? { recursive: "true" }
                            : undefined,
                    })
                    .json(),
        },
    };
}
