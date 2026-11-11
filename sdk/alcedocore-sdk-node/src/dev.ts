export function createDevResource(ky: any) {
    return {
        /**
         * POST /api/dev/request-id — mint a plugin request id.
         * The response is bare `{ request_id }` (not JSend), so the
         * unwrapJSend hook passes it through untouched.
         */
        requestId: (slug: string, options?: any) =>
            ky.post("dev/request-id", { json: { slug }, ...options }).json(),
    };
}
