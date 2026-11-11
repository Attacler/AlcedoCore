import { KyInstance } from "ky";

export interface UserOption {
    id: string;
    email?: string;
    display_name?: string;
}

export function createLookupResource(ky: KyInstance) {
    return {
        /**
         * App-scoped user options for the `user` field input. Users are not
         * exposed through the generic items API; this endpoint enforces the
         * users collection's read policy.
         */
        users: (options?: any) =>
            ky
                .get("app/lookup/users", options)
                .json<{ data: UserOption[] }>(),
    };
}
