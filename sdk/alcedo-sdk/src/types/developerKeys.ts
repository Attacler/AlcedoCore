export interface DeveloperKey {
    id: string;
    name: string;
    key_prefix: string;
    is_active: boolean;
    created_at: string;
    last_used_at: string | null;
}
export type DeveloperKeyWithRawKey = {
    raw_key: string;
} & DeveloperKey;
