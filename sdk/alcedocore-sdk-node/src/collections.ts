import { KyInstance } from "ky";

export interface CollectionField {
    default?: any;
    display_name: string;
    name: string;
    type: string;
    required: boolean;
    unique: boolean;
    is_system: boolean;
}

export interface Collection {
    created_at: string;
    display_name?: string;
    fields: CollectionField[];
    is_system: boolean;
    name: string;
    updated_at: string;
}

export interface UnregisteredColumn {
    name: string;
    field_type: string;
    related_collection?: string;
    related_app?: string;
    required: boolean;
}

export interface UnregisteredTable {
    name: string;
    columns: UnregisteredColumn[];
}

export interface ImportCollectionField {
    name: string;
    display_name?: string;
    input_component?: string;
    display_component?: string;
}

export interface ImportCollectionRequest {
    table: string;
    display_name?: string;
    fields?: ImportCollectionField[];
}

export interface UnregisteredFieldsResponse {
    collection: string;
    fields: UnregisteredColumn[];
}

export interface ImportFieldsRequest {
    fields?: ImportCollectionField[];
}

export function createCollectionsResource(ky: KyInstance) {
    return {
        list: (options?: any) =>
            ky
                .get("app/collections", options)
                .json<{ collections: Collection[] }>(),
        get: (name: string, options?: any) =>
            ky
                .get(`app/collections/${encodeURIComponent(name)}`, options)
                .json<Collection>(),
        create: (data: any, options?: any) =>
            ky
                .post("app/collections", { json: data, ...options })
                .json<Collection>(),
        update: (name: string, data: any, options?: any) =>
            ky
                .put(`app/collections/${encodeURIComponent(name)}`, {
                    json: data,
                    ...options,
                })
                .json<{ success: true }>(),
        delete: (name: string, options?: any) =>
            ky
                .delete(`app/collections/${encodeURIComponent(name)}`, options)
                .json<{ deleted: true }>(),
        schema: (options?: any) => ky.get("app/collections/schema", options).json(),
        schemaTs: (options?: any) =>
            ky.get("app/collections/schema/ts", options).json(),
        listUnregistered: (options?: any) =>
            ky
                .get("app/collections/unregistered", options)
                .json<{ tables: UnregisteredTable[] }>(),
        importCollection: (data: ImportCollectionRequest, options?: any) =>
            ky
                .post("app/collections/import", { json: data, ...options })
                .json<Collection>(),
        listUnregisteredFields: (name: string, options?: any) =>
            ky
                .get(
                    `app/collections/${encodeURIComponent(name)}/unregistered-fields`,
                    options,
                )
                .json<UnregisteredFieldsResponse>(),
        importFields: (name: string, data: ImportFieldsRequest, options?: any) =>
            ky
                .post(
                    `app/collections/${encodeURIComponent(name)}/import-fields`,
                    { json: data, ...options },
                )
                .json<Collection>(),

        // Layouts
        listLayouts: (name: string, options?: any) =>
            ky
                .get(`app/collections/${encodeURIComponent(name)}/layouts`, options)
                .json(),
        createLayout: (name: string, data: any, options?: any) =>
            ky
                .post(`app/collections/${encodeURIComponent(name)}/layouts`, {
                    json: data,
                    ...options,
                })
                .json(),
        updateLayout: (
            name: string,
            layoutId: string,
            data: any,
            options?: any,
        ) =>
            ky
                .put(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}`,
                    { json: data, ...options },
                )
                .json(),
        deleteLayout: (name: string, layoutId: string, options?: any) =>
            ky
                .delete(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}`,
                    options,
                )
                .json(),
        getLayoutRoles: (name: string, layoutId: string, options?: any) =>
            ky
                .get(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/roles`,
                    options,
                )
                .json(),
        setLayoutRoles: (
            name: string,
            layoutId: string,
            roleIds: string[],
            options?: any,
        ) =>
            ky
                .put(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/roles`,
                    { json: { role_ids: roleIds }, ...options },
                )
                .json(),
        getResolvedLayout: (name: string, options?: any) =>
            ky
                .get(`app/collections/${encodeURIComponent(name)}/layout`, options)
                .json(),

        // Layout-scoped sections
        listLayoutSections: (name: string, layoutId: string, options?: any) =>
            ky
                .get(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections`,
                    options,
                )
                .json(),
        createLayoutSection: (
            name: string,
            layoutId: string,
            data: any,
            options?: any,
        ) =>
            ky
                .post(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections`,
                    { json: data, ...options },
                )
                .json(),
        updateLayoutSection: (
            name: string,
            layoutId: string,
            sectionId: string,
            data: any,
            options?: any,
        ) =>
            ky
                .put(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections/${encodeURIComponent(sectionId)}`,
                    { json: data, ...options },
                )
                .json(),
        deleteLayoutSection: (
            name: string,
            layoutId: string,
            sectionId: string,
            options?: any,
        ) =>
            ky
                .delete(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections/${encodeURIComponent(sectionId)}`,
                    options,
                )
                .json(),
        batchReorderSections: (
            name: string,
            layoutId: string,
            sections: any[],
            options?: any,
        ) =>
            ky
                .patch(
                    `app/collections/${encodeURIComponent(name)}/layouts/${encodeURIComponent(layoutId)}/sections`,
                    { json: { sections }, ...options },
                )
                .json(),

        // Saved views
        listViews: (name: string, options?: any) =>
            ky
                .get(`app/collections/${encodeURIComponent(name)}/views`, options)
                .json(),
        createView: (name: string, data: any, options?: any) =>
            ky
                .post(`app/collections/${encodeURIComponent(name)}/views`, {
                    json: data,
                    ...options,
                })
                .json(),
        updateView: (name: string, viewId: string, data: any, options?: any) =>
            ky
                .put(
                    `app/collections/${encodeURIComponent(name)}/views/${encodeURIComponent(viewId)}`,
                    { json: data, ...options },
                )
                .json(),
        deleteView: (name: string, viewId: string, options?: any) =>
            ky
                .delete(
                    `app/collections/${encodeURIComponent(name)}/views/${encodeURIComponent(viewId)}`,
                    options,
                )
                .json(),
        setDefaultView: (name: string, viewId: string, options?: any) =>
            ky
                .put(
                    `app/collections/${encodeURIComponent(name)}/views/${encodeURIComponent(viewId)}/default`,
                    options,
                )
                .json(),
        getCreatePolicy: (name: string, options?: any) =>
            ky
                .get(`app/collections/${encodeURIComponent(name)}/$create`, options)
                .json(),
    };
}
