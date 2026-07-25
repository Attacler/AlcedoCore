export interface MediaFile {
    id: string;
    filename: string;
    mime_type: string;
    size_bytes: number;
    alt_text: string | null;
    created_at: string;
    updated_at: string;
    download_url: string;
}

export interface FileFolder {
    id: string;
    name: string;
    parent_id: string | null;
    created_by: string | null;
    created_at: string;
    updated_at: string;
}

export interface ListFilesParameters {
    limit?: number;
    offset?: number;
    search?: string;
    mime_type?: string;
    folder_id?: string;
}
