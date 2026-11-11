export interface DeveloperKey {
    id: string;
    name: string;
    version_id: number;
    key_prefix: string;
    is_active: boolean;
    created_at: string;
    last_used_at: string | null;
    raw_key?: string;
}
export type DeveloperKeyWithRawKey = {
    raw_key: string;
} & DeveloperKey;
