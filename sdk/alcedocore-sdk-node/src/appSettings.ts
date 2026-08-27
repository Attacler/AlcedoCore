import { KyInstance } from "ky";

export interface Settings {
    catch_all_plugin_slug: string;
    favicon_url: string;
    logo_url: string;
    site_name: string;
}

export interface SettingsUpdateResponse {
    description?: string;
    key: string;
    updated_at: string;
    value?: string;
}

export interface SettingsBatchUpdateResponse {
    count: number;
    success: boolean;
}

export function createAppSettingsResource(ky: KyInstance) {
    return {
        list: (options?: any) => ky.get("settings", options).json<Settings>(),
        update: (key: string, value: any, options?: any) =>
            ky
                .put(`settings/${encodeURIComponent(key)}`, {
                    json: { value },
                    ...options,
                })
                .json<SettingsUpdateResponse>(),
        batch: (settings: Record<string, any>, options?: any) =>
            ky
                .post("settings/batch", { json: { settings }, ...options })
                .json<SettingsBatchUpdateResponse>(),
    };
}
