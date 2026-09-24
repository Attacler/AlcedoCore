export function createItemsResource(ky: any) {
    return {
        list: (
            name: string,
            params?: Record<string, string>,
            options?: Record<string, any>,
        ) =>
            ky
                .get(
                    `${name == "users" ? "platform/users" : "app/items" + `/${encodeURIComponent(name)}`}`,
                    { searchParams: params, ...options },
                )
                .json(),

        create: (name: string, data: any, options?: Record<string, any>) =>
            ky
                .post(`app/items/${encodeURIComponent(name)}`, {
                    json: data,
                    ...options,
                })
                .json(),

        update: (
            name: string,
            data: { filter: any; update: any },
            options?: Record<string, any>,
        ) =>
            ky
                .put(`app/items/${encodeURIComponent(name)}`, {
                    json: data,
                    ...options,
                })
                .json(),

        delete: (
            name: string,
            data: { filter?: any; pk_values?: any[] },
            options?: Record<string, any>,
        ) =>
            ky
                .delete(`app/items/${encodeURIComponent(name)}`, {
                    json: data,
                    ...options,
                })
                .json(),

        get: (
            name: string,
            id: string,
            params?: Record<string, string>,
            options?: Record<string, any>,
        ) =>
            ky
                .get(
                    `app/items/${encodeURIComponent(name)}/${encodeURIComponent(id)}`,
                    { searchParams: params, ...options },
                )
                .json(),

        patch: (
            name: string,
            id: string,
            data: any,
            options?: Record<string, any>,
        ) =>
            ky
                .patch(
                    `app/items/${encodeURIComponent(name)}/${encodeURIComponent(id)}`,
                    { json: data, ...options },
                )
                .json(),

        query: (name: string, data: any, options?: Record<string, any>) => {
            const params = new URLSearchParams();
            if (data.filter) params.set("filter", JSON.stringify(data.filter));
            if (data.limit) params.set("limit", String(data.limit));
            if (data.offset) params.set("offset", String(data.offset));
            if (data.fields) params.set("fields", data.fields.join(","));
            if (data.sort && data.sort.length > 0) {
                params.set("sort", data.sort[0].field);
                if (data.sort[0].order || data.sort[0].direction) {
                    params.set(
                        "order",
                        data.sort[0].order || data.sort[0].direction,
                    );
                }
            }
            return ky
                .get(`app/items/${encodeURIComponent(name)}`, {
                    searchParams: params,
                    ...options,
                })
                .json();
        },

        grouped: (name: string, data: any, options?: Record<string, any>) =>
            ky
                .post(`app/items/${encodeURIComponent(name)}/grouped`, {
                    json: data,
                    ...options,
                })
                .json(),

        references: (name: string, id: string, options?: Record<string, any>) =>
            ky
                .get(
                    `app/items/${encodeURIComponent(name)}/${encodeURIComponent(id)}/references`,
                    options,
                )
                .json(),
    };
}
