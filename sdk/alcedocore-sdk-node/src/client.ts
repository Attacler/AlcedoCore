import ky, { type BeforeRequestHook } from "ky";
import { createPluginsResource } from "./plugins.js";
import { createHealthResource } from "./health.js";
import { createMigrationsResource } from "./migrations.js";
import { createSettingsResource } from "./settings.js";
import { createUsageResource } from "./usage.js";
import { createKvResource } from "./kv.js";
import { createDbResource } from "./db.js";
import { createSchemaResource } from "./schema.js";
import { createLogsResource } from "./logs.js";
import { createAuthResource } from "./auth.js";
import { createUsersResource } from "./users.js";
import { createRolesResource } from "./roles.js";
import { createPoliciesResource } from "./policies.js";
import { createCollectionsResource } from "./collections.js";
import { createItemsResource } from "./items.js";
import { createFilesResource } from "./files.js";
import { createRegistriesResource } from "./registries.js";
import { createActivityLogsResource } from "./activityLogs.js";
import { createAppSettingsResource } from "./appSettings.js";
import { createDeveloperApiKeysResource } from "./developerApiKeys.js";

const DEFAULT_TIMEOUT = 30_000;

export interface ClientOptions {
    timeout?: number;
    requestId?: string;
    retry?: {
        limit?: number;
        delay?: (attempt: number) => number;
    };
}

export function createClient(baseUrl: string, options: ClientOptions = {}) {
    const kyInstance = ky.create({
        baseUrl: baseUrl.replace(/\/$/, ""),
        prefix: "/api",
        timeout: options.timeout ?? DEFAULT_TIMEOUT,
        retry: {
            limit: options.retry?.limit ?? 3,
            delay:
                options.retry?.delay ??
                ((attempt: number) => Math.pow(2, attempt) * 1000),
            methods: ["get", "post", "put", "delete", "patch"] as any,
            statusCodes: [408, 413, 429, 500, 502, 503, 504],
        },
        hooks: {
            beforeRequest: [
                ((state: any) => {
                    const rid = state.options?.requestId ?? options.requestId;

                    if (rid) state.request.headers.set("X-Request-ID", rid);
                }) as BeforeRequestHook,
            ],
        },
    });

    function createRequest(ky2: typeof kyInstance) {
        return async (method: string, path: string, opts?: any) => {
            return (ky2 as any)[method](path, opts);
        };
    }

    return {
        plugins: createPluginsResource(kyInstance),
        health: createHealthResource(kyInstance),
        activityLogs: createActivityLogsResource(kyInstance),
        appSettings: createAppSettingsResource(kyInstance),
        developerApiKeys: createDeveloperApiKeysResource(kyInstance),
        registries: createRegistriesResource(kyInstance),
        migrations: createMigrationsResource(kyInstance),
        settings: createSettingsResource(kyInstance),
        usage: createUsageResource(kyInstance),
        kv: createKvResource(kyInstance),
        db: createDbResource(kyInstance),
        schema: createSchemaResource(kyInstance),
        logs: createLogsResource(kyInstance),
        auth: createAuthResource(kyInstance),
        users: createUsersResource(kyInstance),
        roles: createRolesResource(kyInstance),
        policies: createPoliciesResource(kyInstance),
        collections: createCollectionsResource(kyInstance),
        items: createItemsResource(kyInstance),
        files: createFilesResource(kyInstance),
        request: createRequest(kyInstance),
    };
}
