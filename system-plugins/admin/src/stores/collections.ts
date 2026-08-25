import { defineStore } from "pinia";
import { ref } from "vue";
import { useAlcedoClient } from "../composables/useAlcedoClient";
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
    relationship_type?: "one_to_one" | "many_to_one" | "one_to_many";
    display_field?: string;
    inline_parent_fields?: string[];
    options?: FieldOption[] | Record<string, any>;
    is_system?: boolean;
    hidden?: boolean;
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
    display_options?: Record<string, any>;
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
    const collections = ref<Collection[]>([]),
        loading = ref(false),
        error = ref<string | null>(null),
        currentCollection = ref<Collection | null>(null),
        collectionCache: { [key: string]: Promise<Collection> } = {};

    let fetchCollectionsPromise: Promise<void> | null = null; // Cache collection responses

    async function fetchCollections(force = false) {
        if (fetchCollectionsPromise && !force) return fetchCollectionsPromise;
        fetchCollectionsPromise = withAsyncHandlingVoid(
            loading,
            error,
            async () => {
                const response = (await client.collections.list()) as any;
                collections.value =
                    response.data?.collections || response.collections || [];
            },
        ).finally(() => {
            fetchCollectionsPromise = null;
        });
        return fetchCollectionsPromise;
    }

    async function getCollection(name: string): Promise<Collection> {
        if (name in collectionCache) return await collectionCache[name];

        collectionCache[name] = new Promise(async (res, rej) => {
            try {
                const response = (await client.collections.get(name)) as any;
                const data = response.data || response;

                currentCollection.value = data;
                res(data);
            } catch (e) {
                rej(e);
            }
        });

        return await collectionCache[name];
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
        data: { fields?: FieldDefinition[]; display_name?: string | null },
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
    ): Promise<CollectionLayout[]> {
        try {
            const response = (await client.collections.listLayouts(
                collectionName,
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

    async function getResolvedLayout(collectionName: string): Promise<any> {
        const response = (await client.collections.getResolvedLayout(
            collectionName,
        )) as any;
        return response.data || response;
    }

    async function listLayoutSections(
        collectionName: string,
        layoutId: string,
    ): Promise<any[]> {
        const response = (await client.collections.listLayoutSections(
            collectionName,
            layoutId,
        )) as any;
        const data = response.data || response;
        return data.sections || [];
    }

    async function createLayoutSection(
        collectionName: string,
        layoutId: string,
        data: any,
    ): Promise<any> {
        return (await client.collections.createLayoutSection(
            collectionName,
            layoutId,
            data,
        )) as any;
    }

    async function updateLayoutSection(
        collectionName: string,
        layoutId: string,
        sectionId: string,
        data: any,
    ): Promise<any> {
        return (await client.collections.updateLayoutSection(
            collectionName,
            layoutId,
            sectionId,
            data,
        )) as any;
    }

    async function deleteLayoutSection(
        collectionName: string,
        layoutId: string,
        sectionId: string,
    ): Promise<any> {
        return (await client.collections.deleteLayoutSection(
            collectionName,
            layoutId,
            sectionId,
        )) as any;
    }

    async function batchReorderSections(
        collectionName: string,
        layoutId: string,
        sections: any[],
    ): Promise<any> {
        return (await client.collections.batchReorderSections(
            collectionName,
            layoutId,
            sections,
        )) as any;
    }

    return {
        collections,
        loading,
        error,
        currentCollection,
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
