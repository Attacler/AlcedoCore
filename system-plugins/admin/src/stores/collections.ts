import { defineStore } from "pinia";
import { ref } from "vue";
import { useAlcedoClient } from "../composables/useAlcedoClient";
import { useAppContextStore } from "@/stores/appContext";
import { withAsyncHandlingVoid } from "../utils/asyncUtils";

export type FieldType =
    | "string"
    | "text"
    | "int"
    | "float"
    | "datetime"
    | "uuid"
    | "relationship"
    | "boolean"
    | "file";

export interface FieldOption {
    label: string;
    value: string;
}

export interface FieldDefinition {
    name: string;
    display_name?: string;
    type: FieldType;
    required?: boolean;
    unique?: boolean;
    default_value?: string | null;
    display_type?: string;
    input_component?: string;
    display_component?: string;
    ordinal_position?: number;
    related_collection?: string;
    related_app?: string | null;
    relationship_type?: "one_to_one" | "many_to_one" | "one_to_many";
    display_field?: string;
    inline_parent_fields?: string[];
    options?: FieldOption[] | Record<string, any>;
    is_system?: boolean;
    child_field?: string;
    _key?: string;
    _tempName?: string;
}

export interface CollectionSection {
    _key?: string;
    _field_columns?: Record<string, number>;
    _columns?: number;
    id?: string;
    collection_name?: string;
    name: string;
    section_type: "field_group" | "relational";
    relation_field?: string | null;
    related_app?: string | null;
    view_type?: string;
    default_filter?: any;
    display_fields?: string[] | null;
    item_limit?: number;
    ordinal_position?: number;
    created_at?: string;
    updated_at?: string;
}

export interface Collection {
    name: string;
    display_name?: string;
    fields: FieldDefinition[];
    created_at?: string;
    updated_at?: string;
    is_system?: boolean;
}

export interface CollectionLayout {
    id: string;
    collection_name: string;
    name: string;
    is_default: boolean;
    ordinal_position: number;
    created_at?: string;
    updated_at?: string;
}

export const useCollectionsStore = defineStore("collections", () => {
    const { client } = useAlcedoClient();
    const appContext = useAppContextStore();
    // Scope cache keys by the active app context so switching apps never
    // serves a previous app's collection definition (e.g. both apps have
    // `notes` with different fields).
    const cacheScope = () =>
        `${appContext.appSlug ?? "global"}:${appContext.version ?? "default"}`;
    const collections = ref<Collection[]>([]),
        loading = ref(false),
        error = ref<string | null>(null),
        currentCollection = ref<Collection | null>(null),
        collectionCache: { [key: string]: Promise<Collection> } = {};

    // Field names explicitly deleted in the current editing session. Only these
    // are sent as `removed_fields` on save, so layout/section-only saves never
    // drop columns.
    const deletedFieldNames = ref<string[]>([]);

    function markFieldDeleted(name: string) {
        if (!deletedFieldNames.value.includes(name)) {
            deletedFieldNames.value.push(name);
        }
    }

    function clearDeletedFields() {
        deletedFieldNames.value = [];
    }

    // Per-scope collection-list promises so switching apps refetches.
    const fetchCollectionsPromises: Record<string, Promise<void> | null> = {};

    async function fetchCollections(
        force = false,
        target?: { app?: string | null; version?: string | null },
    ) {
        const scope = `${target?.app ?? appContext.appSlug ?? "global"}:${target?.version ?? appContext.version ?? "default"}`;
        if (fetchCollectionsPromises[scope] && !force)
            return fetchCollectionsPromises[scope];
        fetchCollectionsPromises[scope] = withAsyncHandlingVoid(
            loading,
            error,
            async () => {
                const response = (await client.collections.list({
                    app: target?.app ?? undefined,
                    version: target?.version ?? undefined,
                })) as any;
                collections.value = response.collections || [];
            },
        ).finally(() => {
            fetchCollectionsPromises[scope] = null;
        });
        return fetchCollectionsPromises[scope];
    }

    async function getCollection(
        name: string,
        ignoreCache = false,
        target?: { app?: string | null; version?: string | null },
    ): Promise<Collection> {
        const scope = target
            ? `${target?.app ?? appContext.appSlug ?? "global"}:${target?.version ?? appContext.version ?? "default"}`
            : cacheScope();
        const key = `${scope}:${name}`;
        if (!ignoreCache && key in collectionCache)
            return await collectionCache[key];

        // Clear any stale definition from a previous app context before the
        // scoped fetch resolves.
        currentCollection.value = null;

        collectionCache[key] = new Promise(async (res, rej) => {
            try {
                const response = (await client.collections.get(name, {
                    app: target?.app ?? undefined,
                    version: target?.version ?? undefined,
                })) as any;
                const data = response.data || response;

                currentCollection.value = data;
                res(data);
            } catch (e) {
                rej(e);
            }
        });

        return await collectionCache[key];
    }

    async function createCollection(data: {
        name: string;
    }): Promise<Collection> {
        const response = (await client.collections.create(data)) as any;
        await fetchCollections(true);
        return response.data || response;
    }

    async function updateCollection(
        name: string,
        data: {
            fields?: FieldDefinition[];
            removed_fields?: string[];
            display_name?: string | null;
        },
    ): Promise<Collection> {
        const response = (await client.collections.update(name, data)) as any;
        await fetchCollections(true);
        return response.data || response;
    }

    async function deleteCollection(name: string) {
        await client.collections.delete(name);
        await fetchCollections(true);
    }

    async function fetchReferences(
        collectionName: string,
        itemId: string,
    ): Promise<any[]> {
        const response = (await client.items.references(
            collectionName,
            itemId,
        )) as any;
        return response.data?.references || response.references || [];
    }

    async function listLayouts(
        collectionName: string,
        target?: { app?: string | null; version?: string | null },
    ): Promise<CollectionLayout[]> {
        try {
            const response = (await client.collections.listLayouts(
                collectionName,
                {
                    app: target?.app ?? undefined,
                    version: target?.version ?? undefined,
                },
            )) as any;
            const data = response.data || response;

            return data.layouts || [];
        } catch (e) {
            return [];
        }
    }

    async function createLayout(
        collectionName: string,
        name: string,
    ): Promise<any> {
        return (await client.collections.createLayout(collectionName, {
            name,
        })) as any;
    }

    async function updateLayout(
        collectionName: string,
        layoutId: string,
        data: any,
    ): Promise<any> {
        return (await client.collections.updateLayout(
            collectionName,
            layoutId,
            data,
        )) as any;
    }

    async function deleteLayout(
        collectionName: string,
        layoutId: string,
    ): Promise<any> {
        return (await client.collections.deleteLayout(
            collectionName,
            layoutId,
        )) as any;
    }

    async function getLayoutRoles(
        collectionName: string,
        layoutId: string,
    ): Promise<any[]> {
        const response = (await client.collections.getLayoutRoles(
            collectionName,
            layoutId,
        )) as any;
        const data = response.data || response;
        return data.roles || [];
    }

    async function setLayoutRoles(
        collectionName: string,
        layoutId: string,
        roleIds: string[],
    ): Promise<any> {
        return (await client.collections.setLayoutRoles(
            collectionName,
            layoutId,
            roleIds,
        )) as any;
    }

    async function getResolvedLayout(
        collectionName: string,
        target?: { app?: string | null; version?: string | null },
    ): Promise<any> {
        const response = (await client.collections.getResolvedLayout(
            collectionName,
            {
                app: target?.app ?? undefined,
                version: target?.version ?? undefined,
            },
        )) as any;
        return response.data || response;
    }

    async function listLayoutSections(
        collectionName: string,
        layoutId: string,
        target?: { app?: string | null; version?: string | null },
    ): Promise<any[]> {
        const response = (await client.collections.listLayoutSections(
            collectionName,
            layoutId,
            {
                app: target?.app ?? undefined,
                version: target?.version ?? undefined,
            },
        )) as any;
        const data = response.data || response;
        return data.sections || [];
    }

    async function createLayoutSection(
        collectionName: string,
        layoutId: string,
        data: any,
        target?: { app?: string | null; version?: string | null },
    ): Promise<any> {
        return (await client.collections.createLayoutSection(
            collectionName,
            layoutId,
            data,
            {
                app: target?.app ?? undefined,
                version: target?.version ?? undefined,
            },
        )) as any;
    }

    async function updateLayoutSection(
        collectionName: string,
        layoutId: string,
        sectionId: string,
        data: any,
        target?: { app?: string | null; version?: string | null },
    ): Promise<any> {
        return (await client.collections.updateLayoutSection(
            collectionName,
            layoutId,
            sectionId,
            data,
            {
                app: target?.app ?? undefined,
                version: target?.version ?? undefined,
            },
        )) as any;
    }

    async function deleteLayoutSection(
        collectionName: string,
        layoutId: string,
        sectionId: string,
        target?: { app?: string | null; version?: string | null },
    ): Promise<any> {
        return (await client.collections.deleteLayoutSection(
            collectionName,
            layoutId,
            sectionId,
            {
                app: target?.app ?? undefined,
                version: target?.version ?? undefined,
            },
        )) as any;
    }

    async function batchReorderSections(
        collectionName: string,
        layoutId: string,
        sections: any[],
        target?: { app?: string | null; version?: string | null },
    ): Promise<any> {
        return (await client.collections.batchReorderSections(
            collectionName,
            layoutId,
            sections,
            {
                app: target?.app ?? undefined,
                version: target?.version ?? undefined,
            },
        )) as any;
    }

    return {
        collections,
        loading,
        error,
        currentCollection,
        deletedFieldNames,
        markFieldDeleted,
        clearDeletedFields,
        fetchCollections,
        getCollection,
        createCollection,
        updateCollection,
        deleteCollection,
        fetchReferences,
        listLayouts,
        createLayout,
        updateLayout,
        deleteLayout,
        getLayoutRoles,
        setLayoutRoles,
        getResolvedLayout,
        listLayoutSections,
        createLayoutSection,
        updateLayoutSection,
        deleteLayoutSection,
        batchReorderSections,
    };
});
