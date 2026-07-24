import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { useAlcedoClient } from "../composables/useAlcedoClient";

export interface CategoryDef {
    id: string;
    label: string;
    icon: string;
    description: string;
}

export const CATEGORIES: CategoryDef[] = [
    {
        id: "general",
        label: "General",
        icon: "settings",
        description: "Core application settings",
    },
    {
        id: "menu",
        label: "Menu Builder",
        icon: "list",
        description: "Edit sidebar menu structure",
    },
    {
        id: "branding",
        label: "Branding",
        icon: "palette",
        description: "Appearance and branding",
    },
    {
        id: "activity",
        label: "Activity Log",
        icon: "history",
        description: "View system and collection activity",
    },
    {
        id: "session",
        label: "Session",
        icon: "lock",
        description: "Session management and security",
    },
    {
        id: "developer",
        label: "Developer",
        icon: "code",
        description: "Developer options and diagnostics",
    },
];

export interface SettingMeta {
    key: string;
    label: string;
    description: string;
    category: CategoryDef["id"];
    type: "string" | "url" | "number" | "boolean" | "select" | "file-image";
    required: boolean;
    defaultValue: any;
    options?: { value: string; label: string }[];
    min?: number;
    max?: number;
}

export const SETTING_META: Record<string, SettingMeta> = {
    site_name: {
        key: "site_name",
        label: "Site Name",
        description:
            "The application name displayed in the browser title and sidebar header",
        category: "branding",
        type: "string",
        required: true,
        defaultValue: "",
    },
    logo_url: {
        key: "logo_url",
        label: "Logo",
        description:
            "Logo image displayed in the sidebar header. Upload a PNG, JPG, or SVG file.",
        category: "branding",
        type: "file-image",
        required: false,
        defaultValue: "",
    },
    favicon_url: {
        key: "favicon_url",
        label: "Favicon",
        description:
            "Favicon displayed in the browser tab. Upload a PNG or ICO file (typically 32x32 or 16x16).",
        category: "branding",
        type: "file-image",
        required: false,
        defaultValue: "",
    },
    catch_all_plugin_slug: {
        key: "catch_all_plugin_slug",
        label: "Catch-all Plugin",
        description:
            "Plugin slug that receives all unmatched requests. Empty = disabled.",
        category: "general",
        type: "select",
        required: false,
        defaultValue: "",
    },
};

export const useSettingsStore = defineStore("settings", () => {
    const { client } = useAlcedoClient();

    // ── State ──
    const settings = ref<Record<string, any>>({});
    const localEdits = ref<Record<string, any>>({});
    const savedValues = ref<Record<string, any>>({});
    const loading = ref(false);
    const error = ref<string | null>(null);
    const searchQuery = ref("");
    const dynamicOptions = ref<
        Record<string, { value: string; label: string }[]>
    >({
        catch_all_plugin_slug: [{ label: "(Disabled)", value: "" }],
    });

    // ── Computed ──
    const isDirty = computed(() => {
        for (const key of Object.keys(localEdits.value)) {
            if (localEdits.value[key] !== savedValues.value[key]) {
                return true;
            }
        }
        return false;
    });

    const filteredKeys = computed(() => {
        const q = searchQuery.value.trim().toLowerCase();
        if (!q) return null;

        const keys = new Set<string>();
        // Check known SETTING_META keys
        for (const [key, meta] of Object.entries(SETTING_META)) {
            if (
                key.toLowerCase().includes(q) ||
                meta.label.toLowerCase().includes(q) ||
                meta.description.toLowerCase().includes(q)
            ) {
                keys.add(key);
            }
        }
        // Check dynamic keys from settings
        for (const key of Object.keys(settings.value)) {
            if (!keys.has(key) && key.toLowerCase().includes(q)) {
                keys.add(key);
            }
        }
        return Array.from(keys);
    });

    const hasSettings = computed(() => Object.keys(settings.value).length > 0);

    // ── Helpers ──
    function getSettingValue(key: string): any {
        if (key in localEdits.value) return localEdits.value[key];
        if (key in settings.value) return settings.value[key];
        return SETTING_META[key]?.defaultValue;
    }

    function validateValue(key: string): string | null {
        const meta = SETTING_META[key];
        if (!meta) return null;
        const value = getSettingValue(key);

        if (
            meta.required &&
            (value === null || value === undefined || value === "")
        ) {
            return `${meta.label} is required`;
        }

        if (
            meta.type === "url" &&
            value &&
            typeof value === "string" &&
            value.trim()
        ) {
            try {
                new URL(value);
            } catch {
                return "Invalid URL format";
            }
        }

        if (meta.type === "select" && value) {
            const validOptions = dynamicOptions.value[key] || meta.options;
            // Only validate when we have the full options (not just the default placeholder)
            if (validOptions && validOptions.length > 1) {
                const validValues = validOptions.map((o) => o.value);
                if (!validValues.includes(value)) {
                    return "Invalid selection";
                }
            }
        }

        if (
            meta.type === "number" &&
            value !== null &&
            value !== undefined &&
            value !== ""
        ) {
            if (isNaN(Number(value))) return "Must be a number";
            const num = Number(value);
            if (meta.min !== undefined && num < meta.min) {
                return `Must be between ${meta.min} and ${meta.max}`;
            }
            if (meta.max !== undefined && num > meta.max) {
                return `Must be between ${meta.min} and ${meta.max}`;
            }
        }

        return null;
    }

    // ── Actions ──
    async function fetchSettings() {
        loading.value = true;
        error.value = null;
        try {
            const response = (await client.appSettings.list()) as Record<
                string,
                any
            >;
            settings.value = response?.data || response || {};
            savedValues.value = { ...settings.value };
            localEdits.value = {};

            // Fetch plugins for dynamic dropdowns (e.g., catch_all_plugin_slug)
            try {
                const pluginsRes = await client.plugins.list();
                const plugins =
                    pluginsRes?.data?.plugins || pluginsRes?.plugins || [];
                dynamicOptions.value["catch_all_plugin_slug"] = [
                    { label: "(Disabled)", value: "" },
                    ...plugins
                        .filter((p: any) => p.slug && p.plugin_type != "static")
                        .map((p: any) => ({
                            label: p.display_name || p.slug,
                            value: p.slug,
                        })),
                ];
            } catch (e) {
                console.error(
                    "[SETTINGS] Failed to fetch plugins for dropdown:",
                    e,
                );
                dynamicOptions.value["catch_all_plugin_slug"] = [
                    { label: "(Disabled)", value: "" },
                ];
            }
        } catch (e) {
            error.value =
                e instanceof Error ? e.message : "Failed to fetch settings";
        } finally {
            loading.value = false;
        }
    }

    function updateLocalValue(key: string, value: any) {
        localEdits.value[key] = value;
    }

    function getLocalValue(key: string): any {
        return key in localEdits.value
            ? localEdits.value[key]
            : settings.value[key];
    }

    async function saveSetting(key: string): Promise<boolean> {
        const value = getLocalValue(key);
        await client.appSettings.update(key, value);
        settings.value[key] = value;
        savedValues.value[key] = value;
        delete localEdits.value[key];
        return true;
    }

    function cancelAll() {
        localEdits.value = {};
    }

    async function resetToDefaults() {
        const defaults: Record<string, any> = {};
        for (const [key, meta] of Object.entries(SETTING_META)) {
            defaults[key] = meta.defaultValue;
        }
        await client.appSettings.batch(defaults);
        await fetchSettings();
    }

    function getCategorySettings(categoryId: string): string[] {
        const allKeys = Object.keys(SETTING_META);
        const categoryKeys = allKeys.filter(
            (key) => SETTING_META[key]?.category === categoryId,
        );

        if (filteredKeys.value !== null) {
            return categoryKeys.filter((key) =>
                filteredKeys.value!.includes(key),
            );
        }

        return categoryKeys;
    }

    return {
        // State
        settings,
        localEdits,
        savedValues,
        loading,
        error,
        searchQuery,
        dynamicOptions,
        // Computed
        isDirty,
        filteredKeys,
        hasSettings,
        // Actions
        fetchSettings,
        updateLocalValue,
        getLocalValue,
        saveSetting,
        cancelAll,
        resetToDefaults,
        getCategorySettings,
        // Helpers
        validateValue,
        getSettingValue,
    };
});
